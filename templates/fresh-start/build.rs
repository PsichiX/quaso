use quaso::assets::AssetCooker;

fn main() {
    println!("cargo::rerun-if-changed=./assets/");
    println!("cargo::rerun-if-changed=./assets.pack");
    let mut cooker = AssetCooker::default().with_basic_recipes();
    cooker.cook("./assets/", "asset").unwrap();
    let package = cooker.into_package();
    std::fs::write("./assets.pack", package.encode().unwrap()).unwrap();
}
