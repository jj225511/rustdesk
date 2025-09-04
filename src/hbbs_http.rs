use hbb_common::log;
use reqwest::blocking::Response;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

#[cfg(feature = "flutter")]
pub mod account;
pub mod downloader;
mod http_client;
pub mod record_upload;
pub mod sync;
pub use http_client::create_http_client;
pub use http_client::create_http_client_async;

#[derive(Debug)]
pub enum HbbHttpResponse<T> {
    ErrorFormat,
    Error(String),
    DataTypeFormat,
    Data(T),
}

impl<T: DeserializeOwned> TryFrom<Response> for HbbHttpResponse<T> {
    type Error = reqwest::Error;

    fn try_from(resp: Response) -> Result<Self, <Self as TryFrom<Response>>::Error> {
        let status = resp.status();
        if !status.is_success() {
            log::error!("HTTP error: {}", status);
            return Ok(Self::Error(format!("HTTP error: {}", status)));
        }
        let full = resp.bytes()?;
        let map = match serde_json::from_slice::<Map<String, Value>>(&full) {
            Ok(m) => m,
            Err(e) => {
                log::error!("Response format error: {:?}", e);
                log::error!("Response content: {}", String::from_utf8_lossy(&full));
                return Ok(Self::Error(format!("Response format error: {:?}", e)));
            }
        };
        if let Some(error) = map.get("error") {
            if let Some(err) = error.as_str() {
                Ok(Self::Error(err.to_owned()))
            } else {
                Ok(Self::ErrorFormat)
            }
        } else {
            match serde_json::from_value(Value::Object(map)) {
                Ok(v) => Ok(Self::Data(v)),
                Err(_) => Ok(Self::DataTypeFormat),
            }
        }
    }
}
