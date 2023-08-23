#![no_std]
extern crate alloc;
#[macro_use]
extern crate sgx_tstd;

use enclave_environment::Environment;
use enclave_runtime::setup_runtime;
use light_client::MapLightClientRegistry;

setup_runtime!({
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .unwrap();
    Environment::new(build_lc_registry())
});

fn build_lc_registry() -> MapLightClientRegistry {
    let mut registry = MapLightClientRegistry::new();
    tendermint_lc::register_implementations(&mut registry);
    registry
}
