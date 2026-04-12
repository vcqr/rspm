pub mod proto {
    pub mod rspm {
        tonic::include_proto!("rspm");
    }
}

pub use proto::rspm::*;
