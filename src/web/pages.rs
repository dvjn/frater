use super::{AppState, authenticate, csrf_from_token, error::WebError, new_csrf_value, views};
use crate::domain::{DomainError, Password, Principal};
use axum::{
    Form,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, Uri, header},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::CookieJar;
use maud::Markup;
use serde::Deserialize;

pub(super) async fn root(State(state): State<AppState>, jar: CookieJar) -> Response {
    let csrf = state.csrf_value(&jar);
    let signed_in = authenticate(&state, &jar, csrf).await.is_ok();
    views::home::page(signed_in).into_response()
}

#[derive(Default, Deserialize)]
pub(super) struct LoginQuery {
    return_to: Option<String>,
    verified: Option<String>,
    reset: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct LoginForm {
    email: String,
    password: String,
    csrf: String,
    return_to: Option<String>,
}

pub(super) fn valid_return_to(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.len() > 4 * 1024 {
        return None;
    }
    let uri: Uri = value.parse().ok()?;
    if uri.scheme().is_some() || uri.authority().is_some() || uri.query().is_none() {
        return None;
    }
    if uri.path() == "/oauth/authorize" {
        return Some(value.to_owned());
    }
    if uri.path() != "/oauth/device" {
        return None;
    }
    let query = uri.query()?;
    let parameters = url::form_urlencoded::parse(query.as_bytes()).collect::<Vec<_>>();
    if parameters.len() != 1
        || parameters[0].0 != "user_code"
        || crate::domain::normalize_user_code(&parameters[0].1).is_err()
    {
        return None;
    }
    Some(value.to_owned())
}

pub(super) async fn login_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<LoginQuery>,
) -> Response {
    let return_to = valid_return_to(query.return_to.as_deref());
    let open = state.domain.account().registration_enabled();
    let notice = if query.verified.is_some() {
        Some("Email verified. Sign in to continue.")
    } else if query.reset.is_some() {
        Some("Password reset. Sign in with the new password.")
    } else {
        None
    };
    if authenticate(&state, &jar, None).await.is_ok() {
        if let Some(csrf) = state.csrf_value(&jar)
            && authenticate(&state, &jar, Some(csrf)).await.is_ok()
        {
            return Redirect::to(return_to.as_deref().unwrap_or("/")).into_response();
        }
        let session = jar.get(state.session_cookie_name()).map(|v| v.value());
        if let Some(session) = session
            && let Ok(token) = state.domain.auth().rotate_csrf(session).await
        {
            return (
                jar.add(state.csrf_cookie(token.expose())),
                Redirect::to(return_to.as_deref().unwrap_or("/")),
            )
                .into_response();
        }
        let token = new_csrf_value();
        let page = views::login::page(&token, "", return_to.as_deref(), false, open, notice);
        return (
            jar.add(state.expired_session_cookie())
                .add(state.csrf_cookie(&token)),
            page,
        )
            .into_response();
    }
    match state.csrf_value(&jar).map(ToOwned::to_owned) {
        Some(token) => views::login::page(&token, "", return_to.as_deref(), false, open, notice)
            .into_response(),
        None => {
            let token = new_csrf_value();
            let page = views::login::page(&token, "", return_to.as_deref(), false, open, notice);
            (jar.add(state.csrf_cookie(&token)), page).into_response()
        }
    }
}

fn csrf_page(state: &AppState, jar: CookieJar, render: impl Fn(&str) -> Markup) -> Response {
    match state.csrf_value(&jar).map(ToOwned::to_owned) {
        Some(token) => render(&token).into_response(),
        None => {
            let token = new_csrf_value();
            let page = render(&token);
            (jar.add(state.csrf_cookie(&token)), page).into_response()
        }
    }
}

#[derive(Deserialize)]
pub(super) struct RegisterForm {
    email: String,
    password: String,
    csrf: String,
}

#[derive(Deserialize)]
pub(super) struct VerifyForm {
    email: String,
    code: String,
    csrf: String,
}

fn require_open_registration(state: &AppState) -> Result<(), WebError> {
    if state.domain.account().registration_enabled() {
        Ok(())
    } else {
        Err(crate::domain::DomainError::NotFound.into())
    }
}

pub(super) async fn register_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Response, WebError> {
    require_open_registration(&state)?;
    Ok(csrf_page(&state, jar, |token| {
        views::register::page(token, "", None)
    }))
}

pub(super) async fn register_submit(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(input): Form<RegisterForm>,
) -> Result<Response, WebError> {
    require_open_registration(&state)?;
    csrf_from_token(&state, &input.csrf, &jar)?;
    let password = Password::new(input.password)
        .map_err(|_| crate::domain::DomainError::InvalidInput("invalid password"));
    let registered = match password {
        Ok(password) => {
            state
                .domain
                .account()
                .register(&input.email, &password)
                .await
        }
        Err(error) => Err(error),
    };
    match registered {
        // The response is the same whether or not the address already exists.
        Ok(()) => Ok(views::register::sent(&input.csrf, &input.email, false).into_response()),
        Err(crate::domain::DomainError::InvalidInput(message)) => Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            views::register::page(&input.csrf, &input.email, Some(message)),
        )
            .into_response()),
        Err(error) => Err(error.into()),
    }
}

pub(super) async fn verify_submit(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(input): Form<VerifyForm>,
) -> Result<Response, WebError> {
    require_open_registration(&state)?;
    csrf_from_token(&state, &input.csrf, &jar)?;
    match state
        .domain
        .account()
        .verify_email(&input.email, &input.code)
        .await
    {
        Ok(()) => Ok(Redirect::to("/login?verified=1").into_response()),
        Err(crate::domain::DomainError::InvalidCredentials) => Ok((
            StatusCode::UNAUTHORIZED,
            views::register::sent(&input.csrf, &input.email, true),
        )
            .into_response()),
        Err(error) => Err(error.into()),
    }
}

#[derive(Deserialize)]
pub(super) struct ResetRequestForm {
    email: String,
    csrf: String,
}

#[derive(Deserialize)]
pub(super) struct ResetConfirmForm {
    email: String,
    code: String,
    password: String,
    password_confirm: String,
    csrf: String,
}

pub(super) async fn reset_page(State(state): State<AppState>, jar: CookieJar) -> Response {
    csrf_page(&state, jar, |token| views::reset::page(token, ""))
}

pub(super) async fn reset_request_submit(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(input): Form<ResetRequestForm>,
) -> Result<Response, WebError> {
    csrf_from_token(&state, &input.csrf, &jar)?;
    // The response is the same whether or not the address has an account.
    state
        .domain
        .account()
        .request_password_reset(&input.email)
        .await?;
    Ok(views::reset::sent(&input.csrf, &input.email, None).into_response())
}

pub(super) async fn reset_confirm_submit(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(input): Form<ResetConfirmForm>,
) -> Result<Response, WebError> {
    csrf_from_token(&state, &input.csrf, &jar)?;
    // A typed password is never echoed back, so a mismatch has to be caught
    // before the code is spent on it.
    if input.password != input.password_confirm {
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            views::reset::sent(
                &input.csrf,
                &input.email,
                Some("The two passwords are not the same."),
            ),
        )
            .into_response());
    }
    let password = Password::new(input.password)
        .map_err(|_| crate::domain::DomainError::InvalidInput("invalid password"));
    let outcome = match password {
        Ok(password) => {
            state
                .domain
                .account()
                .reset_password(&input.email, &input.code, &password)
                .await
        }
        Err(error) => Err(error),
    };
    let (status, error) = match outcome {
        Ok(()) => return Ok(Redirect::to("/login?reset=1").into_response()),
        Err(crate::domain::DomainError::InvalidCredentials) => {
            (StatusCode::UNAUTHORIZED, "That code is invalid or expired.")
        }
        Err(crate::domain::DomainError::InvalidInput(message)) => {
            (StatusCode::UNPROCESSABLE_ENTITY, message)
        }
        Err(error) => return Err(error.into()),
    };
    Ok((
        status,
        views::reset::sent(&input.csrf, &input.email, Some(error)),
    )
        .into_response())
}

fn user_agent(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
}

pub(super) async fn login_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Form(input): Form<LoginForm>,
) -> Result<Response, WebError> {
    // The form carries a random double-submit token in both a __Host cookie
    // and the body, so it does not depend on browser-specific Origin header
    // behavior.
    csrf_from_token(&state, &input.csrf, &jar)?;
    let return_to = valid_return_to(input.return_to.as_deref());
    let password = Password::new(input.password).map_err(|_| WebError::invalid_credentials());
    let issued = match password {
        Ok(password) => state
            .domain
            .auth()
            .login(&input.email, &password, user_agent(&headers))
            .await
            .map_err(|error| match error {
                crate::domain::DomainError::InvalidCredentials
                | crate::domain::DomainError::InvalidInput(_) => WebError::invalid_credentials(),
                other => other.into(),
            }),
        Err(error) => Err(error),
    };
    match issued {
        Ok(issued) => {
            let jar = jar
                .add(state.session_cookie(issued.token()))
                .add(state.csrf_cookie(issued.csrf()));
            Ok((jar, Redirect::to(return_to.as_deref().unwrap_or("/"))).into_response())
        }
        Err(error) if error.is_invalid_credentials() => {
            let page = views::login::page(
                &input.csrf,
                &input.email,
                return_to.as_deref(),
                true,
                state.domain.account().registration_enabled(),
                None,
            );
            Ok((StatusCode::UNAUTHORIZED, page).into_response())
        }
        Err(error) => Err(error),
    }
}

#[derive(Deserialize)]
pub(super) struct LogoutForm {
    csrf: String,
    return_to: Option<String>,
}

pub(super) async fn logout_submit(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(input): Form<LogoutForm>,
) -> Result<Response, WebError> {
    csrf_from_token(&state, &input.csrf, &jar)?;
    let principal = authenticate(&state, &jar, Some(&input.csrf)).await?;
    state.domain.auth().logout(&principal).await?;
    let target = match valid_return_to(input.return_to.as_deref()) {
        Some(return_to) => format!(
            "/login?{}",
            url::form_urlencoded::Serializer::new(String::new())
                .append_pair("return_to", &return_to)
                .finish()
        ),
        None => "/".to_owned(),
    };
    Ok((
        jar.add(state.expired_session_cookie())
            .add(state.expired_csrf_cookie()),
        Redirect::to(&target),
    )
        .into_response())
}

#[derive(Default, Deserialize)]
pub(super) struct AccountQuery {
    changed: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ChangePasswordForm {
    csrf: String,
    current_password: String,
    new_password: String,
}

#[derive(Deserialize)]
pub(super) struct AccountActionForm {
    csrf: String,
}

fn to_login() -> Response {
    Redirect::to("/login").into_response()
}

async fn account_principal(state: &AppState, jar: &CookieJar) -> Option<(Principal, String)> {
    let csrf = state.csrf_value(jar)?.to_owned();
    let principal = authenticate(state, jar, Some(&csrf)).await.ok()?;
    Some((principal, csrf))
}

async fn render_account(
    state: &AppState,
    headers: &HeaderMap,
    principal: &Principal,
    csrf: &str,
    notice: Option<&str>,
    error: Option<&str>,
) -> Result<Markup, WebError> {
    let issuer = state
        .origin
        .effective_origin(headers)
        .map_err(|()| WebError::forbidden_request())?;
    let sessions = state.domain.auth().list_sessions(principal).await?;
    let apps = state
        .domain
        .oauth()
        .list_connected_clients(principal, &issuer)
        .await?;
    Ok(views::account::page(csrf, &sessions, &apps, notice, error))
}

pub(super) async fn account_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Query(query): Query<AccountQuery>,
) -> Result<Response, WebError> {
    let Some((principal, csrf)) = account_principal(&state, &jar).await else {
        return Ok(to_login());
    };
    let notice = query
        .changed
        .is_some()
        .then_some("Your password changed. Your other sessions ended.");
    Ok(
        render_account(&state, &headers, &principal, &csrf, notice, None)
            .await?
            .into_response(),
    )
}

pub(super) async fn account_password_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Form(input): Form<ChangePasswordForm>,
) -> Result<Response, WebError> {
    csrf_from_token(&state, &input.csrf, &jar)?;
    let Ok(principal) = authenticate(&state, &jar, Some(&input.csrf)).await else {
        return Ok(to_login());
    };
    let changed = match (
        Password::new(input.current_password),
        Password::new(input.new_password),
    ) {
        (Ok(current), Ok(new)) => {
            state
                .domain
                .change_password(&principal, &current, &new)
                .await
        }
        (Err(_), _) => Err(DomainError::InvalidCredentials),
        (_, Err(_)) => Err(DomainError::InvalidInput("invalid password")),
    };
    let (status, error) = match changed {
        Ok(()) => return Ok(Redirect::to("/account?changed=1").into_response()),
        Err(DomainError::InvalidCredentials) => (
            StatusCode::UNAUTHORIZED,
            "Your current password is not correct.",
        ),
        Err(DomainError::InvalidInput(message)) => (StatusCode::UNPROCESSABLE_ENTITY, message),
        Err(error) => return Err(error.into()),
    };
    let page = render_account(&state, &headers, &principal, &input.csrf, None, Some(error)).await?;
    Ok((status, page).into_response())
}

pub(super) async fn account_session_revoke(
    Path(id): Path<uuid::Uuid>,
    State(state): State<AppState>,
    jar: CookieJar,
    Form(input): Form<AccountActionForm>,
) -> Result<Response, WebError> {
    csrf_from_token(&state, &input.csrf, &jar)?;
    let Ok(principal) = authenticate(&state, &jar, Some(&input.csrf)).await else {
        return Ok(to_login());
    };
    let current = principal.session_id() == Some(id);
    state.domain.auth().revoke_session(&principal, id).await?;
    if current {
        return Ok((
            jar.add(state.expired_session_cookie())
                .add(state.expired_csrf_cookie()),
            Redirect::to("/login"),
        )
            .into_response());
    }
    Ok(Redirect::to("/account").into_response())
}

pub(super) async fn account_app_revoke(
    Path(client_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Form(input): Form<AccountActionForm>,
) -> Result<Response, WebError> {
    csrf_from_token(&state, &input.csrf, &jar)?;
    let Ok(principal) = authenticate(&state, &jar, Some(&input.csrf)).await else {
        return Ok(to_login());
    };
    let issuer = state
        .origin
        .effective_origin(&headers)
        .map_err(|()| WebError::forbidden_request())?;
    state
        .domain
        .oauth()
        .revoke_client_grants(&principal, &issuer, &client_id)
        .await?;
    Ok(Redirect::to("/account").into_response())
}
