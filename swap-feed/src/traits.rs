use crate::rate::Rate;

pub trait LatestRate {
    type Error: std::error::Error + Send + Sync + 'static;

    fn latest_rate(&mut self) -> Result<Rate, Self::Error>;
}

// Future: Allow for different price feed sources
pub trait PriceFeed: Sized {
    type Error: std::error::Error + Send + Sync + 'static;
    type Update;

    fn connect(
        url: url::Url,
    ) -> impl std::future::Future<Output = Result<Self, Self::Error>> + Send;
    fn next_update(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Self::Update, Self::Error>> + Send;
}
