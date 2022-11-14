use crate::{
    enclave::HostStoreTxManager, Enclave, EnclaveCommandAPI, EnclavePrimitiveAPI, EnclaveProtoAPI,
};
use store::memory::MemStore;

impl<'e> HostStoreTxManager<MemStore> for Enclave<'e, MemStore> {}
impl<'e> EnclavePrimitiveAPI<MemStore> for Enclave<'e, MemStore> {}
impl<'e> EnclaveCommandAPI<MemStore> for Enclave<'e, MemStore> {}
impl<'e> EnclaveProtoAPI<MemStore> for Enclave<'e, MemStore> {}