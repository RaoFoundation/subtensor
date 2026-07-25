//! Builds the runtime WASM blob when compiling with `std`.
//!
//! With the `metadata-hash` feature, embeds a metadata hash for token `TAO`
//! (9 decimals) so clients can verify metadata against the runtime binary.

fn main() {
    #[cfg(all(feature = "std", not(feature = "metadata-hash")))]
    {
        substrate_wasm_builder::WasmBuilder::new()
            .with_current_project()
            .export_heap_base()
            .import_memory()
            .build();
    }
    #[cfg(all(feature = "std", feature = "metadata-hash"))]
    {
        substrate_wasm_builder::WasmBuilder::new()
            .with_current_project()
            .export_heap_base()
            .import_memory()
            .enable_metadata_hash("TAO", 9)
            .build();
    }
}
