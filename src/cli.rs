//! Command line: without arguments the binary serves; `set-password <name> [--admin]` replaces
//! legacy `scripts/set-password.ts`.
//!
//! `ensure_first_admin` only acts on an empty user table, so a database whose admin password is
//! lost is otherwise only reachable through SQL. Unlike the script, the password is not an
//! argument (it would land in the shell history and in `ps`): it is asked for twice on a terminal,
//! or read as the first line of stdin (`echo "$PW" | autoreverse-server set-password oliver`).

use std::io::{BufRead, IsTerminal};

use sqlx::PgPool;

use crate::auth::directory::{SetPasswordOutcome, set_password_or_create, valid_password};

pub const USAGE: &str = "Aufruf: autoreverse-server [set-password <name> [--admin]]";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Serve,
    SetPassword { name: String, admin: bool },
}

/// The command from the arguments after the program name. `Err` carries the message for stderr.
pub fn parse(args: &[String]) -> Result<Command, String> {
    match args.split_first() {
        None => Ok(Command::Serve),
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
        ] {
            assert_eq!(parse(&args(bad)), Err(USAGE.into()), "{bad:?}");
        }
    }
}
