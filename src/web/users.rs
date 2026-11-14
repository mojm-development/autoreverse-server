//! Settings → Nutzer (admins) and Settings → Sicherheit (everyone) — legacy
//! `routes/settings/users` and `routes/settings/security`.
//!
//! Like Bibliotheken, each form posts back to its page as a named action (`?/createUser`,
//! `?/toggleAdmin`, `?/setPassword`, `?/deleteUser`) and the page renders again with the outcome.

use std::collections::HashMap;

use askama::Template;
use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::Uri;
use axum::response::Html;
use axum::{Router, routing::get};

use super::format::numeric_date;
use super::layout::Shell;
use super::settings::{Section, settings_shell, web_user};
use crate::api::me::change_own_password;
use crate::auth::directory::{
    DirectoryError, NAME_MAX, PASSWORD_MAX, PASSWORD_MIN, add_user, delete_user, find_user,
    js_length, list_users, reset_password, set_admin,
};
use crate::auth::{ApiUser, WebAdmin, WebUser};
use crate::error::{WebError, WebResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/settings/users", get(users).post(users_action))
        .route("/settings/security", get(security).post(security_action))
}

/// A line under the heading after an action: green for done, red for refused.
pub enum Flash {
    Success(String),
    Error(String),
}

pub struct UserLine {
    pub id: i32,
    pub name: String,
    pub is_admin: bool,
    pub is_self: bool,
    pub last_seen: String,
}

#[derive(Template)]
#[template(path = "pages/settings/users.html")]
struct UsersPage {
    shell: Shell,
    sections: Vec<&'static Section>,
    users: Vec<UserLine>,
    flash: Option<Flash>,
    /// The create form stays open (and keeps the name) after a refused attempt.
    create_open: bool,
    create_name: String,
    password_min: usize,
}

async fn render_users(
    state: &AppState,
    admin: &WebAdmin,
    flash: Option<Flash>,
    create_name: Option<String>,
) -> WebResult<Html<String>> {
    let uri: Uri = "/settings/users".parse().expect("static uri");
    let (shell, sections) = settings_shell(state, &web_user(admin), &uri).await?;
    let users = list_users(&state.db)
        .await?
        .into_iter()
        .map(|u| UserLine {
            is_self: u.id == admin.id,
            id: u.id,
            name: u.name,
            is_admin: u.is_admin,
            last_seen: numeric_date(u.last_seen_at),
        })
        .collect();
    let page = UsersPage {
        shell,
        sections,
        users,
        flash,
        create_open: create_name.is_some(),
        create_name: create_name.unwrap_or_default(),
        password_min: PASSWORD_MIN,
    };
    Ok(Html(page.render()?))
}

async fn users(State(state): State<AppState>, admin: WebAdmin) -> WebResult<Html<String>> {
    render_users(&state, &admin, None, None).await
}

fn form_of(body: &Bytes) -> HashMap<String, String> {
    serde_urlencoded::from_bytes(body).unwrap_or_default()
}

fn field<'a>(form: &'a HashMap<String, String>, key: &str) -> &'a str {
    form.get(key).map_or("", String::as_str)
}

/// The password rule as a sentence for the forms.
fn password_problem(password: &str) -> Option<String> {
    let len = js_length(password);
    if len < PASSWORD_MIN {
        Some(format!(
            "Das Passwort muss mindestens {PASSWORD_MIN} Zeichen haben."
        ))
    } else if len > PASSWORD_MAX {
        Some(format!(
            "Das Passwort darf höchstens {PASSWORD_MAX} Zeichen haben."
        ))
    } else {
        None
    }
}

fn directory_message(e: DirectoryError, last_admin: &str) -> WebResult<String> {
    Ok(match e {
        DirectoryError::NotFound => "Unbekannter Nutzer.".into(),
        DirectoryError::LastAdmin => last_admin.into(),
        DirectoryError::SelfDelete => "Das eigene Konto kann nicht gelöscht werden.".into(),
        DirectoryError::NameTaken => "Nutzername ist bereits vergeben.".into(),
        DirectoryError::Db(e) => return Err(WebError::from(e)),
    })
}

/// The user a row form names; `None` if the id is broken or the user is gone.
async fn target(
    state: &AppState,
    form: &HashMap<String, String>,
) -> WebResult<Option<(i32, String)>> {
    let Ok(id) = field(form, "userId").parse::<i32>() else {
        return Ok(None);
    };
    Ok(find_user(&state.db, id).await?.map(|u| (u.id, u.name)))
}

async fn users_action(
    State(state): State<AppState>,
    admin: WebAdmin,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> WebResult<Html<String>> {
    let form = form_of(&body);
    let unknown = || Some(Flash::Error("Unbekannter Nutzer.".into()));
    match query.as_deref() {
        Some("/createUser") => {
            let name = field(&form, "name").trim().to_owned();
            let password = field(&form, "password");
            let is_admin = form.contains_key("isAdmin");
            let problem = if name.is_empty() || password.is_empty() {
                Some(format!(
                    "Name und Passwort (min. {PASSWORD_MIN} Zeichen) erforderlich."
                ))
            } else if js_length(&name) > NAME_MAX {
                Some(format!("Der Name darf höchstens {NAME_MAX} Zeichen haben."))
            } else {
                password_problem(password)
            };
            if let Some(problem) = problem {
                return render_users(&state, &admin, Some(Flash::Error(problem)), Some(name)).await;
            }
            match add_user(&state.db, &name, password, is_admin).await {
                Ok(_) => {
                    let done = Flash::Success(format!("„{name}“ angelegt."));
                    render_users(&state, &admin, Some(done), None).await
                }
                Err(e) => {
                    let message = directory_message(e, "")?;
                    render_users(&state, &admin, Some(Flash::Error(message)), Some(name)).await
                }
            }
        }
        Some("/toggleAdmin") => {
            let Some((id, name)) = target(&state, &form).await? else {
                return render_users(&state, &admin, unknown(), None).await;
            };
            let promote = field(&form, "isAdmin") == "true";
            let flash = match set_admin(&state.db, id, promote).await {
                Ok(true) => Flash::Success(format!("„{name}“ ist jetzt Verwalter.")),
                Ok(false) => Flash::Success(format!("„{name}“ ist jetzt Nutzer.")),
                Err(e) => Flash::Error(directory_message(
                    e,
                    "Der letzte Verwalter kann nicht herabgestuft werden.",
                )?),
            };
            render_users(&state, &admin, Some(flash), None).await
        }
        Some("/setPassword") => {
            let Some((id, name)) = target(&state, &form).await? else {
                return render_users(&state, &admin, unknown(), None).await;
            };
            let password = field(&form, "password");
            let flash = match password_problem(password) {
                Some(problem) => Flash::Error(problem),
                None => {
                    let keep = (id == admin.id).then_some(admin.token.as_str());
                    match reset_password(&state.db, id, password, keep).await {
                        Ok(()) => Flash::Success(format!(
                            "Passwort für „{name}“ gesetzt. Andere Sitzungen wurden abgemeldet."
                        )),
                        Err(e) => Flash::Error(directory_message(e, "")?),
                    }
                }
            };
            render_users(&state, &admin, Some(flash), None).await
        }
        Some("/deleteUser") => {
            let Some((id, name)) = target(&state, &form).await? else {
                return render_users(&state, &admin, unknown(), None).await;
            };
            let flash = match delete_user(&state.db, Some(admin.id), id).await {
                Ok(_) => Flash::Success(format!("„{name}“ gelöscht.")),
                Err(e) => Flash::Error(directory_message(
                    e,
                    "Der letzte Verwalter kann nicht gelöscht werden.",
                )?),
            };
            render_users(&state, &admin, Some(flash), None).await
        }
        _ => render_users(&state, &admin, None, None).await,
    }
}

#[derive(Template)]
#[template(path = "pages/settings/security.html")]
struct SecurityPage {
    shell: Shell,
    sections: Vec<&'static Section>,
    flash: Option<Flash>,
    password_min: usize,
}

async fn render_security(
    state: &AppState,
    user: &WebUser,
    flash: Option<Flash>,
) -> WebResult<Html<String>> {
    let uri: Uri = "/settings/security".parse().expect("static uri");
    let (shell, sections) = settings_shell(state, user, &uri).await?;
    Ok(Html(
        SecurityPage {
            shell,
            sections,
            flash,
            password_min: PASSWORD_MIN,
        }
        .render()?,
    ))
}

async fn security(State(state): State<AppState>, user: WebUser) -> WebResult<Html<String>> {
    render_security(&state, &user, None).await
}

/// Legacy's default action: check the current password, set the new one, end the other
/// sessions. The texts are legacy's.
async fn security_action(
    State(state): State<AppState>,
    user: WebUser,
    body: Bytes,
) -> WebResult<Html<String>> {
    let form = form_of(&body);
    let current = field(&form, "currentPassword");
    let new_password = field(&form, "newPassword");
    let flash = if js_length(new_password) < PASSWORD_MIN {
        Flash::Error(format!(
            "Neues Passwort muss mindestens {PASSWORD_MIN} Zeichen haben."
        ))
    } else if js_length(new_password) > PASSWORD_MAX {
        Flash::Error(format!(
            "Neues Passwort darf höchstens {PASSWORD_MAX} Zeichen haben."
        ))
    } else {
        let api_user = ApiUser {
            id: user.id,
            token: user.token.clone(),
        };
        match change_own_password(&state, &api_user, current, new_password).await {
            Ok(()) => Flash::Success("Passwort geändert.".into()),
            Err(e) if e.status == 401 => Flash::Error("Aktuelles Passwort ist falsch.".into()),
            Err(e) if e.status == 404 => Flash::Error("Unbekannter Nutzer.".into()),
            Err(e) => return Err(WebError::Internal(e.detail)),
        }
    };
    render_security(&state, &user, Some(flash)).await
}
