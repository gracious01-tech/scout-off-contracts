use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum ScoutAccessError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    ContractPaused = 3,
    Unauthorized = 4,
    InsufficientFee = 5,
    ScoutNotSubscribed = 6,
    SubscriptionExpired = 7,
    AlreadyContacted = 8,
    InvalidTier = 9,
    Overflow = 10,
    TrialOfferNotFound = 11,
    ProgressCallFailed = 14,
}
