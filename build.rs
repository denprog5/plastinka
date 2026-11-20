fn main() {
    // Иконки exe/трея, манифест и сведения о версии. Страница и картинки встраиваются через include_*.
    println!("cargo:rerun-if-changed=assets");
    embed_resource::compile("assets/app.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
