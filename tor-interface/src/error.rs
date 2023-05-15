use std::fmt;

pub struct Error {
    message: String,
    file: &'static str,
    line: u32,
    function: &'static str,
}

impl Error {
    pub fn new(message: String, file: &'static str, line: u32, function: &'static str) -> Self {
        Self {
            message,
            line,
            function,
            file,
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}({}:{}): {}",
            self.function, self.file, self.line, self.message
        )
    }
}

pub trait ToError {
    fn to_error(self, file: &'static str, line: u32, function: &'static str) -> Error;
}

impl<T> ToError for T
where
    T: std::string::ToString,
{
    fn to_error(self, file: &'static str, line: u32, function: &'static str) -> Error {
        Error {
            message: self.to_string(),
            line,
            function,
            file,
        }
    }
}

impl ToError for Error {
    fn to_error(self, _file: &'static str, _line: u32, _function: &'static str) -> Error {
        self
    }
}

pub type Result<T, E = Error> = core::result::Result<T, E>;

#[macro_export]
macro_rules! function {
    () => {{
        fn f() {}
        fn type_name_of<T>(_: T) -> &'static str {
            std::any::type_name::<T>()
        }
        let name = type_name_of(f);
        &name[..name.len() - 3]
    }};
}

#[macro_export]
macro_rules! to_error {
    ($err:tt) => {{
        let line = std::line!();
        let function = function!();
        let file = std::file!();

        use $crate::error::ToError;
        $err.to_error(file, line, function)
    }};
}

#[macro_export]
macro_rules! bail {
    ($msg:literal) => {
        {
            return Err(to_error!($msg));
        }
    };
    ($err:expr) => {
        {
            return Err(to_error!($err));
        }
    };
    ($fmt:literal, $($arg:tt)*) => {
        {
            let message = std::format!($fmt, $($arg)*);
            return Err(to_error!(message));
        }
    };
}

#[macro_export]
macro_rules! resolve {
    ($result:expr) => {
        match $result {
            Ok(val) => val,
            Err(err) => bail!(err),
        }
    };
}

#[macro_export]
macro_rules! ensure {
    ($condition:expr) => {
        if !($condition as bool) {
            bail!(std::format!("requirement `{}` failed", std::stringify!($condition)));
        }
    };
    ($condition:expr, $msg:literal) => {
        if !($condition as bool) {
            bail!($msg);
        }
    };
    ($condition:expr, $fmt:literal, $($arg:tt)*) => {
        if !($condition as bool) {
            bail!($fmt, $($arg)*);
        }
    };
}

#[macro_export]
macro_rules! ensure_not_null {
    ($ptr:expr) => {
        if $ptr.is_null() {
            bail!(std::format!("`{}` must not be null", std::stringify!($ptr)));
        }
    };
}

#[macro_export]
macro_rules! ensure_equal {
    ($left:expr, $right:expr) => {
        let left_val = $left;
        let right_val = $right;
        if left_val != right_val {
            bail!(std::format!(
                "`{}` must equal `{}` but found left: {:?}, right: {:?}",
                std::stringify!($left),
                std::stringify!($right),
                left_val,
                right_val
            ));
        }
    };
}

#[macro_export]
macro_rules! ensure_ok {
    ($result:expr) => {
        match $result {
            Ok(_) => {}
            Err(err) => bail!(err),
        }
    };
}
