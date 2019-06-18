FROM buildpack-deps:stretch

ENV LLVM_HOME=/usr/local/llvm \
    ISPC_HOME=/usr/local/ispc

RUN set -x \
 && apt-get update \
 && apt-get install -y cmake bison flex \
 && git clone git://github.com/ispc/ispc.git $ISPC_HOME \
 && cd $ISPC_HOME \
 && python alloy.py -b --version=8.0 --git --selfbuild

RUN mkdir build \
 && cmake -DCMAKE_INSTALL_PREFIX=$ISPC_HOME/bin -DCMAKE_CXX_COMPILER=clang++ $ISPC_HOME/root \
 && make \
 && make install
