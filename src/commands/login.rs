use crate::auth;
use crate::error::CliError;

pub fn login() -> Result<(), CliError> {
    auth::login()
}

pub fn logout() -> Result<(), CliError> {
    auth::logout()
}
