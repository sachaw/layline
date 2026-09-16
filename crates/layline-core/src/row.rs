//! The table row macro.

/// Defines a table row with the standard derives, `#[non_exhaustive]`, and a `new` taking every field.
macro_rules! row {
    (
        $(#[$meta:meta])*
        $name:ident<$lt:lifetime> {
            $( $(#[$field_meta:meta])* $field:ident : $ty:ty ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub struct $name<$lt> {
            $( $(#[$field_meta])* pub $field: $ty, )*
        }

        impl<$lt> $name<$lt> {
            /// A row from its fields.
            #[must_use]
            pub const fn new($($field: $ty),*) -> Self {
                Self { $($field),* }
            }
        }
    };
}

pub(crate) use row;
