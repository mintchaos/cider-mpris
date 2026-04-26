pub mod types;

use reqwest::Client;
use std::time::Duration;
use types::*;

const CIDER_BASE_URL: &str = "http://localhost:10767";
const TIMEOUT: Duration = Duration::from_secs(5);

pub struct CiderClient {
    client: Client,
    base_url: String,
    token: String,
}

impl CiderClient {
    pub fn new(token: String) -> Self {
        let client = Client::builder()
            .timeout(TIMEOUT)
            .build()
            .expect("Failed to create HTTP client");
        
        Self {
            client,
            base_url: CIDER_BASE_URL.to_string(),
            token,
        }
    }
    
    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "apptoken",
            self.token.parse().expect("Invalid token"),
        );
        headers
    }
    
    pub async fn is_playing(&self) -> Result<bool, ClientError> {
        let url = format!("{}/api/v1/playback/is-playing", self.base_url);
        let resp = self.client
            .get(&url)
            .headers(self.headers())
            .send()
            .await?;
        Ok(resp.json::<PlaybackStatusResponse>().await?.is_playing)
    }
    
    pub async fn now_playing(&self) -> Result<Option<NowPlayingInfo>, ClientError> {
        let url = format!("{}/api/v1/playback/now-playing", self.base_url);
        let resp = self.client
            .get(&url)
            .headers(self.headers())
            .send()
            .await;
        
        match resp {
            Ok(r) if r.status() == reqwest::StatusCode::NO_CONTENT => Ok(None),
            Ok(r) => {
                match r.json::<NowPlayingResponse>().await {
                    Ok(parsed) => Ok(Some(parsed.info)),
                    Err(e) => {
                        tracing::debug!("Failed to parse now-playing response: {:?}", e);
                        Ok(None) // incomplete response — treat as nothing playing
                    }
                }
            }
            Err(e) if e.is_connect() => Ok(None),
            Err(e) => Err(ClientError::Reqwest(e)),
        }
    }
    
    pub async fn play(&self) -> Result<(), ClientError> {
        let url = format!("{}/api/v1/playback/play", self.base_url);
        self.client.post(&url).headers(self.headers()).send().await?;
        Ok(())
    }
    
    pub async fn pause(&self) -> Result<(), ClientError> {
        let url = format!("{}/api/v1/playback/pause", self.base_url);
        self.client.post(&url).headers(self.headers()).send().await?;
        Ok(())
    }
    
    pub async fn play_pause(&self) -> Result<(), ClientError> {
        let url = format!("{}/api/v1/playback/playpause", self.base_url);
        self.client.post(&url).headers(self.headers()).send().await?;
        Ok(())
    }
    
    pub async fn next(&self) -> Result<(), ClientError> {
        let url = format!("{}/api/v1/playback/next", self.base_url);
        self.client.post(&url).headers(self.headers()).send().await?;
        Ok(())
    }
    
    pub async fn previous(&self) -> Result<(), ClientError> {
        let url = format!("{}/api/v1/playback/previous", self.base_url);
        self.client.post(&url).headers(self.headers()).send().await?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum ClientError {
    #[allow(dead_code)]
    Reqwest(reqwest::Error),
}

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        ClientError::Reqwest(e)
    }
}
