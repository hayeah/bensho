//! The bench's own columns.

/// The user's columns on every CSV row. `columns()` is the header slice,
/// `values()` one string per column in the same order. `()` is the empty row.
///
/// The `row!` macro writes this impl for a plain struct.
pub trait Row {
    fn columns() -> &'static [&'static str];
    fn values(&self) -> Vec<String>;
}

impl Row for () {
    fn columns() -> &'static [&'static str] {
        &[]
    }

    fn values(&self) -> Vec<String> {
        Vec::new()
    }
}

/// A value that can sit in a CSV cell. `Option::None` is the empty cell,
/// which pandas reads as NaN.
pub trait Field {
    fn cell(&self) -> String;
}

macro_rules! display_fields {
    ($($t:ty),*) => {
        $(impl Field for $t {
            fn cell(&self) -> String {
                self.to_string()
            }
        })*
    };
}

display_fields!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64, bool, char, String,
    &str
);

impl<T: Field> Field for Option<T> {
    fn cell(&self) -> String {
        match self {
            Some(v) => v.cell(),
            None => String::new(),
        }
    }
}

/// Declare a row struct and its `Row` impl:
///
/// ```
/// bensho::row! {
///     /// Bytes moved and the engine's own share of the batch.
///     pub struct PacketRow { bytes: u64, engine_ns: u128, note: Option<String> }
/// }
/// ```
///
/// Every field type must implement `Field`. The column names are the field
/// names verbatim.
#[macro_export]
macro_rules! row {
    ($(#[$meta:meta])* $vis:vis struct $name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug)]
        $vis struct $name {
            $(pub $field: $ty),*
        }

        impl $crate::Row for $name {
            fn columns() -> &'static [&'static str] {
                &[$(stringify!($field)),*]
            }

            fn values(&self) -> Vec<String> {
                vec![$($crate::Field::cell(&self.$field)),*]
            }
        }
    };
}
