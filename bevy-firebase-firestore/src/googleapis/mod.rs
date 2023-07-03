pub mod google {
    pub mod api {
        include!("google.api.rs");
    }
    pub mod firestore {
        pub mod v1 {
            include!("google.firestore.v1.rs");
        }
    }
    pub mod r#type {
        include!("google.r#type.rs");
    }
    pub mod rpc {
        include!("google.rpc.rs");
    }
}
