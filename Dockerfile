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
#   /opt/suwayomi/bin/jvm-sandbox.jar
#   /opt/suwayomi/jre/                 （扩展跑在 JVM 里，缺了就没有任何来源）
#   /opt/suwayomi/webui/
# 数据目录单独在 /data（VOLUME）。
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

# --- 2) 扩展沙盒 jar（gradle） -------------------------------------------
# 与 CI 桌面矩阵里那步是同一件事。整个仓库都要 COPY：`jvm-sandbox` 是独立 gradle 工程，
# 而它的 sourceSets 直接引用上一级的共享源码
# （`srcDir("../extension-runtime/src/main/kotlin")`，见 jvm-sandbox/build.gradle.kts）。
FROM eclipse-temurin:25-jdk AS sandbox
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl unzip ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY . .
RUN cd jvm-sandbox && ./gradlew -q jar --no-daemon

# --- 3) 裁剪 JRE 25（jlink） ---------------------------------------------
# 用的是与 `+jre` 桌面包**同一个**脚本、同一份模块白名单，所以容器里的扩展运行环境
# 与桌面包完全一致（含 jdk.httpserver / jdk.crypto.ec，刻意排除 java.desktop）。
# Temurin JDK 24 起按 JEP 493 不再随归档带 jmods/，脚本会自己去 Adoptium 取。
FROM eclipse-temurin:25-jdk AS jre
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl python3 ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY scripts/make-jre.sh /scripts/make-jre.sh
# 只为**本机架构**裁剪：脚本自己会校验「宿主架构 == 目标架构」，不一致直接报错退出。
RUN set -eu; \
    case "$(uname -m)" in \
      aarch64|arm64) A=aarch64 ;; \
      x86_64|amd64)  A=x64 ;; \
      *) echo "不支持的架构：$(uname -m)" >&2; exit 1 ;; \
    esac; \
    bash /scripts/make-jre.sh linux "$A" /jre

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
COPY --from=sandbox /app/jvm-sandbox/build/libs/suwayomi-jvm-sandbox.jar \
                    /opt/suwayomi/bin/jvm-sandbox.jar
COPY --from=jre     /jre                                          /opt/suwayomi/jre
COPY --from=webui   /webui                                        /opt/suwayomi/webui

# 三个路径都显式钉住：默认解析规则能猜对（exe 在 bin/ 下），但容器里写死更省事，
# 也避免将来有人改了 exe 的位置就把 webui 与数据目录一起带偏。
ENV SUWAYOMI_WEBUI_DIR=/opt/suwayomi/webui \
    SUWAYOMI_DATA_DIR=/data \
    SUWAYOMI_PORT=8090 \
    SUWAYOMI_IP=0.0.0.0
RUN mkdir -p /data/autobackup /data/downloads /data/local
VOLUME ["/data"]
EXPOSE 8090
ENTRYPOINT ["/opt/suwayomi/bin/suwayomi-server"]
