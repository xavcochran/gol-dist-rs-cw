fn main() {
    tonic_build::compile_protos("../proto/stub.proto").unwrap();
}
