use std::fmt;
use std::process;

#[derive(Debug)]
pub enum CliError {
    Api { status: u16, message: String },
    Auth(String),
    Config(String),
    Network(String),
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::Api { .. } => 1,
            CliError::Auth(_) => 2,
            CliError::Config(_) => 3,
            CliError::Network(_) => 4,
        }
    }

    pub fn exit(self) -> ! {
        eprintln!("error: {self}");
        process::exit(self.exit_code());
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Api {
                status, message, ..
            } => write!(f, "[{status}] {message}"),
            CliError::Auth(msg) | CliError::Config(msg) | CliError::Network(msg) => {
                write!(f, "{msg}")
            }
        }
    }
}

impl From<reqwest::Error> for CliError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            CliError::Network(format!("request timed out: {err}"))
        } else if err.is_connect() {
            CliError::Network(format!("connection failed: {err}"))
        } else {
            CliError::Network(format!("network error: {err}"))
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        CliError::Config(format!("io error: {err}"))
    }
}

impl From<toml::de::Error> for CliError {
    fn from(err: toml::de::Error) -> Self {
        CliError::Config(format!("config parse error: {err}"))
    }
}

impl From<toml::ser::Error> for CliError {
    fn from(err: toml::ser::Error) -> Self {
        CliError::Config(format!("config write error: {err}"))
    }
}

impl From<serde_json::Error> for CliError {
    fn from(err: serde_json::Error) -> Self {
        CliError::Api {
            status: 0,
            message: format!("failed to parse JSON: {err}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_correct() {
        assert_eq!(
            CliError::Api {
                status: 400,
                message: "bad request".into()
            }
            .exit_code(),
            1
        );
        assert_eq!(CliError::Auth("err".into()).exit_code(), 2);
        assert_eq!(CliError::Config("err".into()).exit_code(), 3);
        assert_eq!(CliError::Network("err".into()).exit_code(), 4);
    }

    #[test]
    fn display_api_error() {
        let err = CliError::Api {
            status: 404,
            message: "resource not found".into(),
        };
        assert_eq!(format!("{err}"), "[404] resource not found");
    }

    #[test]
    fn display_auth_error() {
        let err = CliError::Auth("token expired".into());
        assert_eq!(format!("{err}"), "token expired");
    }

    #[test]
    fn display_config_error() {
        let err = CliError::Config("missing organization".into());
        assert_eq!(format!("{err}"), "missing organization");
    }

    #[test]
    fn display_network_error() {
        let err = CliError::Network("timeout".into());
        assert_eq!(format!("{err}"), "timeout");
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let cli_err = CliError::from(io_err);
        assert_eq!(cli_err.exit_code(), 3);
        assert!(format!("{cli_err}").contains("file not found"));
    }

    #[test]
    fn from_toml_de_error() {
        let toml_err = toml::from_str::<toml::Value>("invalid{{").unwrap_err();
        let cli_err = CliError::from(toml_err);
        assert_eq!(cli_err.exit_code(), 3);
    }

    #[test]
    fn from_serde_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let cli_err = CliError::from(json_err);
        assert_eq!(cli_err.exit_code(), 1);
        match cli_err {
            CliError::Api { message, .. } => assert!(message.contains("failed to parse JSON")),
            _ => panic!("expected Api error"),
        }
    }
}
