macro_rules! cvt {
    ( $fn:expr ) => {{
        let result = unsafe { $fn };
        if result != -1 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }};
}
