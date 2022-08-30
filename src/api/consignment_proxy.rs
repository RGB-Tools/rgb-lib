use reqwest::blocking::{multipart, Client};
use serde::{Deserialize, Serialize};

use std::path::PathBuf;

use crate::error::{Error, InternalError};

const CONSIGNMENT_PROXY_URL: &str = "http://proxy.rgbtools.org";

#[derive(Debug, Deserialize, Serialize)]
pub struct AckResponse {
    pub(crate) success: bool,
    pub(crate) ack: Option<bool>,
    pub(crate) nack: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ConsignmentResponse {
    pub(crate) success: bool,
    pub(crate) consignment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SuccessResponse {
    success: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AckNackRequest {
    blindedutxo: String,
}

pub trait ConsignmentProxy {
    fn get_ack(self, blindedutxo: String) -> Result<AckResponse, Error>;

    fn get_consignment(self, blindedutxo: String) -> Result<ConsignmentResponse, Error>;

    fn post_ack(self, blindedutxo: String) -> Result<SuccessResponse, Error>;

    fn post_nack(self, blindedutxo: String) -> Result<SuccessResponse, Error>;

    fn post_consignment(
        self,
        blindedutxo: String,
        consignment_path: PathBuf,
    ) -> Result<SuccessResponse, Error>;
}

impl ConsignmentProxy for Client {
    fn get_ack(self, blindedutxo: String) -> Result<AckResponse, Error> {
        Ok(self
            .get(format!("{}/ack/{}", CONSIGNMENT_PROXY_URL, blindedutxo))
            .send()
            .map_err(Error::ConsignmentProxy)?
            .json::<AckResponse>()
            .map_err(InternalError::from)?)
    }

    fn get_consignment(self, blindedutxo: String) -> Result<ConsignmentResponse, Error> {
        Ok(self
            .get(format!(
                "{}/consignment/{}",
                CONSIGNMENT_PROXY_URL, blindedutxo
            ))
            .send()
            .map_err(Error::ConsignmentProxy)?
            .json::<ConsignmentResponse>()
            .map_err(InternalError::from)?)
    }

    fn post_nack(self, blindedutxo: String) -> Result<SuccessResponse, Error> {
        let body = AckNackRequest { blindedutxo };
        Ok(self
            .post(format!("{}/nack", CONSIGNMENT_PROXY_URL))
            .json(&body)
            .send()
            .map_err(Error::ConsignmentProxy)?
            .json::<SuccessResponse>()
            .map_err(InternalError::from)?)
    }

    fn post_ack(self, blindedutxo: String) -> Result<SuccessResponse, Error> {
        let body = AckNackRequest { blindedutxo };
        Ok(self
            .post(format!("{}/ack", CONSIGNMENT_PROXY_URL))
            .json(&body)
            .send()
            .map_err(Error::ConsignmentProxy)?
            .json::<SuccessResponse>()
            .map_err(InternalError::from)?)
    }

    fn post_consignment(
        self,
        blindedutxo: String,
        consignment_path: PathBuf,
    ) -> Result<SuccessResponse, Error> {
        let form = multipart::Form::new()
            .text("blindedutxo", blindedutxo)
            .file("consignment", consignment_path)?;
        Ok(self
            .post(format!("{}/consignment", CONSIGNMENT_PROXY_URL))
            .multipart(form)
            .send()
            .map_err(Error::ConsignmentProxy)?
            .json::<SuccessResponse>()
            .map_err(InternalError::from)?)
    }
}
