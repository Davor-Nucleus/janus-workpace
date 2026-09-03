use rodio::OutputStream;

pub struct PlayerService;

impl PlayerService {
    pub fn initialize_audio()
    -> Result<(OutputStream, rodio::OutputStreamHandle), rodio::StreamError> {
        OutputStream::try_default()
    }

    pub fn set_console_title() {
        janus_platform_nucleus::console::set_title("PhonosCore Server");
    }
}
