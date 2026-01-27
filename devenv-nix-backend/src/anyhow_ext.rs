/// Extension trait to convert anyhow::Result to miette::Result
pub trait AnyhowToMiette<T> {
    fn to_miette(self) -> miette::Result<T>;
}

impl<T> AnyhowToMiette<T> for anyhow::Result<T> {
    fn to_miette(self) -> miette::Result<T> {
        // Use {e:#} to show the full error chain including underlying causes
        self.map_err(|e| miette::miette!("{e:#}"))
    }
}
