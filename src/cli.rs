//! Command line: without arguments the binary serves; `set-password <name> [--admin]` replaces
//! legacy `scripts/set-password.ts`; `healthcheck` probes a running server (the Docker
//! `HEALTHCHECK`, since the slim runtime image has neither wget nor curl).
//!
//! `ensure_first_admin` only acts on an empty user table, so a database whose admin password is
//! lost is otherwise only reachable through SQL. Unlike the script, the password is not an
//! argument (it would land in the shell history and in `ps`): it is asked for twice on a terminal,
//! or read as the first line of stdin (`echo "$PW" | autoreverse-server set-password oliver`).

use std::io::{BufRead, IsTerminal};

use sqlx::PgPool;

use crate::auth::directory::{SetPasswordOutcome, set_password_or_create, valid_password};

pub const USAGE: &str = "Aufruf: autoreverse-server [set-password <name> [--admin] | healthcheck]";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Serve,
    SetPassword { name: String, admin: bool },
    Healthcheck,
}

/// The command from the arguments after the program name. `Err` carries the message for stderr.
pub fn parse(args: &[String]) -> Result<Command, String> {
    match args.split_first() {
        None => Ok(Command::Serve),
        Some((first, rest)) if first == "healthcheck" && rest.is_empty() => {
            Ok(Command::Healthcheck)
        }
        Some((first, rest)) if first == "set-password" => {
            let admin = rest.iter().any(|a| a == "--admin");
            let names: Vec<&String> = rest.iter().filter(|a| *a != "--admin").collect();
            match names.as_slice() {
                [name] if !name.is_empty() && !name.starts_with("--") => Ok(Command::SetPassword {
                    name: (*name).clone(),
                    admin,
                }),
                _ => Err(USAGE.into()),
            }
        }
        Some(_) => Err(USAGE.into()),
    }
}

/// The new password: typed twice without echo on a terminal, else the first line of stdin.
pub fn read_new_password() -> Result<String, String> {
    if std::io::stdin().is_terminal() {
        let first = rpassword::prompt_password("Neues Passwort: ").map_err(|e| e.to_string())?;
        let second = rpassword::prompt_password("Wiederholen: ").map_err(|e| e.to_string())?;
        if first != second {
            return Err("Die Passwörter stimmen nicht überein.".into());
        }
        Ok(first)
    } else {
        let mut line = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        Ok(line.trim_end_matches(['\n', '\r']).to_owned())
    }
}

/// Whether the server on `127.0.0.1:port` answers `GET /login` with 200 within a few seconds.
/// Plain HTTP/1.0 over a socket: the probe needs no client library and no TLS, it only runs
/// inside the container next to the server.
pub fn healthcheck(port: u16) -> bool {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;

    let timeout = Duration::from_secs(3);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout) else {
        return false;
    };
    if stream.set_read_timeout(Some(timeout)).is_err()
        || stream
            .write_all(b"GET /login HTTP/1.0\r\nHost: localhost\r\n\r\n")
            .is_err()
    {
        return false;
    }
    let mut head = [0u8; 12];
    stream.read_exact(&mut head).is_ok() && head.starts_with(b"HTTP/1.") && &head[8..12] == b" 200"
}

/// Sets the password (or creates the user) and returns the line to print. `Err` is a message for
/// stderr: a password outside 8–200 characters, or a database failure.
pub async fn set_password(
    db: &PgPool,
    name: &str,
    password: &str,
    admin: bool,
) -> Result<String, String> {
    if !valid_password(password) {
        return Err("Das Passwort muss 8–200 Zeichen haben.".into());
    }
    let suffix = if admin { " (Verwalter)" } else { "" };
    match set_password_or_create(db, name, password, admin).await {
        Ok(SetPasswordOutcome::Updated) => Ok(format!(
            "Passwort für \"{name}\" gesetzt{suffix}. Alle Sitzungen wurden abgemeldet."
        )),
        Ok(SetPasswordOutcome::Created) => Ok(format!("Nutzer \"{name}\" angelegt{suffix}.")),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_serve_and_set_password() {
        assert_eq!(parse(&args(&[])), Ok(Command::Serve));
        assert_eq!(parse(&args(&["healthcheck"])), Ok(Command::Healthcheck));
        assert_eq!(
            parse(&args(&["set-password", "oliver"])),
            Ok(Command::SetPassword {
                name: "oliver".into(),
                admin: false
            })
        );
        assert_eq!(
            parse(&args(&["set-password", "--admin", "Mara Q"])),
            Ok(Command::SetPassword {
                name: "Mara Q".into(),
                admin: true
            })
        );
    }

    #[test]
    fn refuses_anything_else() {
        for bad in [
            &["serve"][..],
            &["set-password"],
            &["set-password", "a", "b"],
            &["set-password", "--admin"],
            &["set-password", "--force", "x"][..],
            &["healthcheck", "x"],
        ] {
            assert_eq!(parse(&args(bad)), Err(USAGE.into()), "{bad:?}");
        }
    }

    /// A one-shot server that answers with `status_line`.
    fn serve_once(status_line: &'static str) -> u16 {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut buf = [0u8; 256];
            let _ = conn.read(&mut buf);
            let _ = conn.write_all(format!("{status_line}\r\n\r\n").as_bytes());
        });
        port
    }

    #[test]
    fn healthcheck_wants_a_200() {
        assert!(healthcheck(serve_once("HTTP/1.1 200 OK")));
        assert!(!healthcheck(serve_once(
            "HTTP/1.1 500 Internal Server Error"
        )));
        let closed = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert!(!healthcheck(closed));
    }
}
