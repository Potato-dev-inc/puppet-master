// wrangler bundles `.wasm` imports as precompiled WebAssembly modules.
declare module "*.wasm" {
  const module: WebAssembly.Module;
  export default module;
}
