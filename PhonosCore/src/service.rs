use rodio::OutputStream;

pub struct PlayerService;

impl PlayerService {
    pub fn initialize_audio()
    -> Result<(OutputStream, rodio::OutputStreamHandle), rodio::StreamError> {
        OutputStream::try_default()
    }

    pub fn set_console_title() {
        #[cfg(windows)]
        {
            use std::ffi::OsStr;
            use std::iter::once;
            use std::os::windows::ffi::OsStrExt;
            use winapi::um::wincon::SetConsoleTitleW;
            let title = "PhonosCore Server";
            let wide: Vec<u16> = OsStr::new(title).encode_wide().chain(once(0)).collect();
            unsafe {
                SetConsoleTitleW(wide.as_ptr());
            }
        }
    }
}
