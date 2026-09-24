# Suwayomi (next) —— 官方容器镜像（headless server）。
#
# 与桌面包同源：同一份 Rust server、同一个沙盒 jar、同一套 jlink 裁剪出来的 JRE 25。
# 差别都是为了容器：
#   * **不含托盘壳** —— 容器里没有 GUI，也没有 webkit2gtk/appindicator 这些系统依赖，
#     装进去只会是个跑不起来的死文件；
#   * 运行镜像只装真正必需的东西（ca-certificates + libssl3t64）。
#
# 目录布局必须与 server 的路径解析约定一致（crates/suwayomi-server/src/lib.rs 的
# resolve_webui_dir / resolve_data_dir / resolve_sandbox_jar，以及
# crates/suwayomi-domain/src/source/sandbox.rs 的 resolve_java）：
# 可执行文件在 `bin/` 下时，`jre/` 在**上一级**、沙盒 jar 与 exe 同级。所以镜像里是
#   /opt/suwayomi/bin/suwayomi-server
#   /opt/suwayomi/bin/ext-runtime.jar
#   /opt/suwayomi/jre/                 （扩展跑在 JVM 里，缺了就没有任何来源）
#   /opt/suwayomi/webui/
# 数据目录单独在 /data（VOLUME）；SQLite 库也在卷内（/data/db），不跟着数据目录走
# —— 数据目录是 WebUI 里可以随时改的设置项，库跟着它走就会把设置本身弄丢。
#
# 本地构建：
#   docker build -t suwayomi-next --build-arg WEBUI_URL=<WebUI zip 地址> .
# 不传 WEBUI_URL 也能构建，只是镜像里没有 WebUI 静态文件（打开只有占位页）。
#
# CI 里由 .github/workflows/build.yml 的 oci job 构建：**每个架构跑在自己的
# runner 上**（linux/amd64 → ubuntu-latest，linux/arm64 → ubuntu-24.04-arm），
# 推 GHCR 后用 imagetools 合成多架构 manifest。不用 QEMU 模拟 —— jlink 不能跨平台
# 生成运行时（见 docs/release.md），模拟编译只是把同一件错事做得更慢。

# --- 1) Rust server ------------------------------------------------------
FROM rust:1.95-slim AS server
# libssl-dev 是编译期必需的：Linux 上 reqwest 走 native-tls → openssl-sys。
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev perl \
    && rm -rf /var/lib/apt/lists/*
# 版本号：CI 的 oci job 把 prep 算好的值传进来（与桌面包同一套）；
# 不传就是 build.rs 的本地默认 —— 上下文里没有 .git，所以会退到兜底值。
ARG VERSION_NAME=""
ARG VERSION_COUNT=""
ARG BUILD_TYPE=""
WORKDIR /app
COPY . .
RUN SUWAYOMI_VERSION_NAME="$VERSION_NAME" \
    SUWAYOMI_VERSION_COUNT="$VERSION_COUNT" \
    SUWAYOMI_BUILD_TYPE="$BUILD_TYPE" \
    cargo build --release -p suwayomi-server

# --- 2) 扩展沙盒 jar -----------------------------------------------------
# 从 Suwayomi-ext-runtime 的 Release 资产取（免鉴权）。曾在镜像里跑 gradle 构建，
# 改成下载之后本仓库不再有 gradle 工程，也不必把整个上下文 COPY 进去。
# CI 会显式传 EXT_RUNTIME_VERSION（由 release.yml 的 prep 解析），默认值只为本机手搓兜底。
ARG EXT_RUNTIME_VERSION="30.1.0"
FROM alpine:3.21 AS sandbox
ARG EXT_RUNTIME_VERSION
RUN apk add --no-cache curl unzip
WORKDIR /out
# -f 必须加：不加会把 404 的 HTML 当 jar 存下来，要到运行时才发现。
# 落地后校验主类，避免「下到了错误页但文件非空」。
RUN curl -fsSL -o ext-runtime.jar \
      "https://github.com/576576/Suwayomi-ext-runtime/releases/download/v${EXT_RUNTIME_VERSION}/ext-runtime-${EXT_RUNTIME_VERSION}.jar" \
    && unzip -l ext-runtime.jar | grep -q 'sandbox/MainKt.class' \
    && echo "ext-runtime ${EXT_RUNTIME_VERSION} 就绪"

# --- 3) 裁剪 JRE 25（从 Suwayomi-ext-runtime 下载） -----------------------
# 裁剪本身在 Suwayomi-ext-runtime 跑（jlink 不能跨平台编译，那边用原生 runner 出六份
# 资产；模块白名单也由沙盒代码决定，必须同仓演进）。这里只按 <V>+<os>+<arch> 取回来。
# 容器与 `+jre` 桌面包吃的是同一份资产，扩展运行环境因此完全一致
# （含 jdk.httpserver / jdk.crypto.ec，刻意排除 java.desktop）。
FROM alpine:3.21 AS jre
ARG EXT_RUNTIME_VERSION
WORKDIR /jre
RUN apk add --no-cache curl \
    && set -eu; \
    case "$(uname -m)" in \
      aarch64|arm64) A=aarch64 ;; \
      x86_64|amd64)  A=x64 ;; \
      *) echo "不支持的架构：$(uname -m)" >&2; exit 1 ;; \
    esac; \
    curl -fsSL --retry 3 -o /tmp/jre.tar.gz \
      "https://github.com/576576/Suwayomi-ext-runtime/releases/download/v${EXT_RUNTIME_VERSION}/ext-runtime-jre-${EXT_RUNTIME_VERSION}-linux-${A}.tar.gz" \
    && tar -xzf /tmp/jre.tar.gz -C /jre \
    && rm -f /tmp/jre.tar.gz \
    && test -x /jre/jre/bin/java

# --- 4) WebUI 静态文件 ---------------------------------------------------
# 取自 Suwayomi-WebUI 的 release 资产（CI 里由 release.yml 的 prep 解析一次，
# 所有 target 复用同一份，避免各架构拉到不同构建）。
FROM debian:trixie-slim AS webui
ARG WEBUI_URL=""
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates unzip \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /webui \
    && if [ -n "$WEBUI_URL" ]; then \
         curl -fsSL --retry 3 -o /tmp/webui.zip "$WEBUI_URL"; \
         unzip -q /tmp/webui.zip -d /webui; \
         rm -f /tmp/webui.zip; \
         test -f /webui/index.html; \
       else \
         echo "未传 WEBUI_URL：镜像里不带 WebUI 静态文件"; \
       fi

# --- 5) 运行镜像 ---------------------------------------------------------
FROM debian:trixie-slim
# libssl3t64 是**必需**的，不是可选：Linux 上 reqwest 走 native-tls → 动态链接
# libssl.so.3（只有 android 换成了 rustls，见 crates/suwayomi-domain/Cargo.toml），
# 缺它连 `--version` 都起不来。trixie 里的包名是带 t64 后缀的那个。
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3t64 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/suwayomi
COPY --from=server  /app/target/release/suwayomi-server           /opt/suwayomi/bin/suwayomi-server
COPY --from=sandbox /out/ext-runtime.jar                          /opt/suwayomi/bin/ext-runtime.jar
COPY --from=jre     /jre/jre                                      /opt/suwayomi/jre
COPY --from=webui   /webui                                        /opt/suwayomi/webui

# 三个路径都显式钉住：默认解析规则能猜对（exe 在 bin/ 下），但容器里写死更省事，
# 也避免将来有人改了 exe 的位置就把 webui 与数据目录一起带偏。
# SUWAYOMI_DB_DIR 必须钉在卷内：不钉的话库会落到 WORKDIR（/opt/suwayomi/db），
# 那是**镜像层**，容器一重建书架就没了。
ENV SUWAYOMI_WEBUI_DIR=/opt/suwayomi/webui \
    SUWAYOMI_DATA_DIR=/data \
    SUWAYOMI_DB_DIR=/data/db \
    SUWAYOMI_PORT=8090 \
    SUWAYOMI_IP=0.0.0.0
RUN mkdir -p /data/db /data/autobackup /data/downloads /data/local
VOLUME ["/data"]
EXPOSE 8090
ENTRYPOINT ["/opt/suwayomi/bin/suwayomi-server"]
