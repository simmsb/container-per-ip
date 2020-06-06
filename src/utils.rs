macro_rules! error_on_error {
    ( $e:expr ) => {
        {
            use log::error;
            match $e {
                Ok(_) => (),
                Err(e) => error!("{} failed with reason: {:?}", stringify!($e), e),
            };
        }
    };
}
