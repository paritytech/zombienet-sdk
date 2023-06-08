// Helper to define states of a type.
// We use an enum with no variants because it can't be constructed by definition.
macro_rules! states {
    ($($ident:ident),*) => {
        $(
            pub enum $ident {}
        )*
    };
}

pub(crate) use states;
