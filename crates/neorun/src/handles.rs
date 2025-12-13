
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AppId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PeerId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RemoteAddr {
    pub peer: PeerId,
    pub target_id: String,
}
