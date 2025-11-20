/// Extension trait to convert anyhow::Result to miette::Result
pub trait AnyhowToMiette<T> {
    fn to_miette(self) -> miette::Result<T>;
}

impl<T> AnyhowToMiette<T> for anyhow::Result<T> {
    fn to_miette(self) -> miette::Result<T> {
        self.map_err(|e| miette::miette!("{e}"))
    }
}
