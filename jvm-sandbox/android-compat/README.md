# android-compat（vendored 源码）

上游：[Suwayomi-Server](https://github.com/Suwayomi/Suwayomi-Server) 的 `AndroidCompat/` 与
`AndroidCompat/Config/`（Mozilla Public License 2.0，Copyright Contributors to the Suwayomi
project，全文见同目录 `LICENSE`）。

相对上游的改动：

- 去掉 `xyz/nulldev/androidcompat/webkit/` 下的 CEF WebView 实现（`CefHelper`、`KcefHelper`、
  `KcefWebSettings`、`KcefWebViewProvider`）与引用它的 `AndroidCompatInitializer`：
  桌面沙盒不注册 WebView provider，也不带 CEF 运行时。
- 去掉 `resources/font/`（37MB，只被 `android.graphics.Typeface` 读取）。
- `app/cash/quickjs/QuickJs` 由 graalvm polyglot 改为 Rhino 实现。

同步上游时按这份列表重新裁剪。
