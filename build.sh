
PKG_CONFIG_PATH=/opt/rh/gcc-toolset-12/root/usr/lib64/pkgconfig:/usr/lib64/pkgconfig
export PKG_CONFIG_PATH

# ./cargo nightly clean
./cargo nightly build

cd sdk

../cargo nightly build

cd cargo-build-sbf/

../../cargo nightly build
