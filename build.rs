fn main() {
    tonic_build::configure()
        .include_file("mod.rs")
        .build_server(false)
        .out_dir("src/googleapis") // you can change the generated code's location
        .compile(
            &["proto/googleapis/google/firestore/v1/firestore.proto"],
            &["proto/googleapis"], // specify the root location to search proto dependencies
        )
        .unwrap();
}
