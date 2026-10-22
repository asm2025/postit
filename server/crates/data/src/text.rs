//! Text-column helpers shared by the repositories: enum text forms and decode errors.

use crate::error::DataError;
use crate::users::ParseEnumError;

/// Wraps a stored value that failed to parse as a database decode error.
pub(crate) fn corrupt<E>(err: E) -> DataError
where
    E: std::error::Error + Send + Sync + 'static,
{
    DataError::Sql(sqlx::Error::Decode(Box::new(err)))
}

/// Parses a stored text column, reporting a bad value as a decode error.
pub(crate) fn parse<T>(value: &str) -> Result<T, DataError>
where
    T: std::str::FromStr<Err = ParseEnumError>,
{
    value.parse().map_err(corrupt)
}

/// Declares an enum stored as text, with `as_str` and `FromStr`.
macro_rules! text_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $name {
            /// The value stored in the column.
            #[must_use]
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::users::ParseEnumError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($text => Ok(Self::$variant),)+
                    other => Err($crate::users::ParseEnumError(other.to_string())),
                }
            }
        }
    };
}

pub(crate) use text_enum;
