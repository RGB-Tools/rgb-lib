use reqwest::{multipart, Body, Client};
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio_util::codec::{BytesCodec, FramedRead};

use std::path::PathBuf;

use crate::{
    error::{Error, InternalError},
    utils::get_runtime_handle,
};

#[derive(Debug, Deserialize, Serialize)]
pub struct InfoResponse {
    pub(crate) version: String,
    pub(crate) uptime: u64,
}

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
pub struct MediaResponse {
    pub(crate) success: bool,
    pub(crate) media: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SuccessResponse {
    success: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AckNackRequest {
    blindedutxo: String,
}

pub trait Proxy {
    fn get_info(self, url: &str) -> Result<InfoResponse, Error>;

    fn get_ack(self, url: &str, blindedutxo: String) -> Result<AckResponse, Error>;

    fn get_consignment(self, url: &str, blindedutxo: String) -> Result<ConsignmentResponse, Error>;

    fn get_media(self, url: &str, attachment_id: String) -> Result<MediaResponse, Error>;

    fn post_ack(self, url: &str, blindedutxo: String) -> Result<SuccessResponse, Error>;

    fn post_nack(self, url: &str, blindedutxo: String) -> Result<SuccessResponse, Error>;

    fn post_consignment(
        self,
        url: &str,
        blindedutxo: String,
        consignment_path: PathBuf,
    ) -> Result<SuccessResponse, Error>;

    fn post_media(
        self,
        url: &str,
        attachment_id: String,
        media_path: PathBuf,
    ) -> Result<SuccessResponse, Error>;
}

impl Proxy for Client {
    fn get_info(self, url: &str) -> Result<InfoResponse, Error> {
        get_runtime_handle()?.block_on(async {
            Ok(self
                .get(format!("{}/getinfo", url))
                .send()
                .await
                .map_err(Error::Proxy)?
                .json::<InfoResponse>()
                .await
                .map_err(InternalError::from)?)
        })
    }

    fn get_ack(self, url: &str, blindedutxo: String) -> Result<AckResponse, Error> {
        get_runtime_handle()?.block_on(async {
            Ok(self
                .get(format!("{}/ack/{}", url, blindedutxo))
                .send()
                .await
                .map_err(Error::Proxy)?
                .json::<AckResponse>()
                .await
                .map_err(InternalError::from)?)
        })
    }

    fn get_consignment(self, url: &str, blindedutxo: String) -> Result<ConsignmentResponse, Error> {
        get_runtime_handle()?.block_on(async {
            Ok(self
                .get(format!("{}/consignment/{}", url, blindedutxo))
                .send()
                .await
                .map_err(Error::Proxy)?
                .json::<ConsignmentResponse>()
                .await
                .map_err(InternalError::from)?)
        })
    }

    fn get_media(self, url: &str, attachment_id: String) -> Result<MediaResponse, Error> {
        get_runtime_handle()?.block_on(async {
            Ok(self
                .get(format!("{}/media/{}", url, attachment_id))
                .send()
                .await
                .map_err(Error::Proxy)?
                .json::<MediaResponse>()
                .await
                .map_err(InternalError::from)?)
        })
    }

    fn post_nack(self, url: &str, blindedutxo: String) -> Result<SuccessResponse, Error> {
        let body = AckNackRequest { blindedutxo };
        get_runtime_handle()?.block_on(async {
            Ok(self
                .post(format!("{}/nack", url))
                .json(&body)
                .send()
                .await
                .map_err(Error::Proxy)?
                .json::<SuccessResponse>()
                .await
                .map_err(InternalError::from)?)
        })
    }

    fn post_ack(self, url: &str, blindedutxo: String) -> Result<SuccessResponse, Error> {
        let body = AckNackRequest { blindedutxo };
        get_runtime_handle()?.block_on(async {
            Ok(self
                .post(format!("{}/ack", url))
                .json(&body)
                .send()
                .await
                .map_err(Error::Proxy)?
                .json::<SuccessResponse>()
                .await
                .map_err(InternalError::from)?)
        })
    }

    fn post_consignment(
        self,
        url: &str,
        blindedutxo: String,
        consignment_path: PathBuf,
    ) -> Result<SuccessResponse, Error> {
        get_runtime_handle()?.block_on(async {
            let file = File::open(consignment_path.clone()).await?;
            let stream = FramedRead::new(file, BytesCodec::new());
            let file_name = consignment_path
                .clone()
                .file_name()
                .map(|filename| filename.to_string_lossy().into_owned())
                .expect("valid file name");
            let consignment_file =
                multipart::Part::stream(Body::wrap_stream(stream)).file_name(file_name);
            let form = multipart::Form::new()
                .text("blindedutxo", blindedutxo)
                .part("consignment", consignment_file);
            Ok(self
                .post(format!("{}/consignment", url))
                .multipart(form)
                .send()
                .await
                .map_err(Error::Proxy)?
                .json::<SuccessResponse>()
                .await
                .map_err(InternalError::from)?)
        })
    }

    fn post_media(
        self,
        url: &str,
        attachment_id: String,
        media_path: PathBuf,
    ) -> Result<SuccessResponse, Error> {
        get_runtime_handle()?.block_on(async {
            let file = File::open(media_path.clone()).await?;
            let stream = FramedRead::new(file, BytesCodec::new());
            let file_name = media_path
                .file_name()
                .map(|filename| filename.to_string_lossy().into_owned())
                .expect("valid file name");
            let media_file =
                multipart::Part::stream(Body::wrap_stream(stream)).file_name(file_name);
            let form = multipart::Form::new()
                .text("attachment_id", attachment_id)
                .part("media", media_file);
            Ok(self
                .post(format!("{}/media", url))
                .multipart(form)
                .send()
                .await
                .map_err(Error::Proxy)?
                .json::<SuccessResponse>()
                .await
                .map_err(InternalError::from)?)
        })
    }
}
