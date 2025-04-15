use quaso::assets::AssetPackage;

fn main() {
    println!("cargo::rerun-if-changed=./assets/");
    println!("cargo::rerun-if-changed=./assets.pack");
    let package = AssetPackage::from_directory("./assets/")
        .unwrap()
        .encode()
        .unwrap();
    std::fs::write("./assets.pack", package).unwrap();
}
