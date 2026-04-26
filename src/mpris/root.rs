use zbus::interface;

pub struct Root {
    pub can_quit: bool,
    pub can_set_fullscreen: bool,
    pub fullscreen: bool,
    pub has_track_list: bool,
    pub identity: String,
    pub can_raise: bool,
    pub desktop_entry: String,
    pub supported_uri_schemes: Vec<String>,
    pub supported_mime_types: Vec<String>,
}

impl Root {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for Root {
    fn default() -> Self {
        Self {
            can_quit: true,
            can_set_fullscreen: false,
            fullscreen: false,
            has_track_list: false,
            identity: "Cider".to_string(),
            can_raise: true,
            desktop_entry: "cider".to_string(),
            supported_uri_schemes: vec![],
            supported_mime_types: vec![],
        }
    }
}

#[interface(interface = "org.mpris.MediaPlayer2")]
impl Root {
    fn quit(&self) {
        tracing::info!("Quit requested");
        std::process::exit(0);
    }

    fn raise(&self) {
        tracing::info!("Raise requested");
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        self.can_quit
    }

    #[zbus(property)]
    fn can_set_fullscreen(&self) -> bool {
        self.can_set_fullscreen
    }

    #[zbus(property)]
    fn fullscreen(&self) -> bool {
        self.fullscreen
    }

    #[zbus(property)]
    fn set_fullscreen(&mut self, value: bool) {
        self.fullscreen = value;
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        self.has_track_list
    }

    #[zbus(property)]
    fn identity(&self) -> &str {
        &self.identity
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        self.can_raise
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        &self.desktop_entry
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        self.supported_uri_schemes.clone()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        self.supported_mime_types.clone()
    }
}
