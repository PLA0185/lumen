# Android APK 发布与签名

每次发布版本 tag 时，GitHub Actions 会构建一个 ARM64 APK，并把它和 Windows 安装包放进同一个 Release。APK 使用稳定的 Android 签名，否则 Android 会拒绝把后续版本安装到现有应用上。这个签名密钥不进仓库，遗失后也不能用新密钥覆盖更新旧 APK。

设置 GitHub 仓库 **Settings → Secrets and variables → Actions**，添加下面四项：

| Secret | 内容 |
| --- | --- |
| `LUMEN_ANDROID_KEYSTORE_BASE64` | `.jks` / `.keystore` 文件的 Base64 文本 |
| `LUMEN_ANDROID_KEY_ALIAS` | keystore 中的 key alias |
| `LUMEN_ANDROID_KEYSTORE_PASSWORD` | keystore 密码 |
| `LUMEN_ANDROID_KEY_PASSWORD` | key 密码 |

不要把 keystore、Base64 文本或密码提交到仓库或发到聊天。

0.4.34 记录的 APK 是 ARM64 Debug 签名包。若要让已安装该 APK 的手机直接升级，应使用构建该包时的同一 keystore。此 Windows 账户当前有 Android 默认 debug keystore，可以运行下面的脚本将文件 Base64 安全复制到剪贴板；脚本不会把内容打印到终端：

```powershell
pwsh -File tools/copy-android-keystore-base64.ps1
```

把剪贴板内容粘贴到 `LUMEN_ANDROID_KEYSTORE_BASE64`。Android 默认 debug keystore 的 alias 通常为 `androiddebugkey`，store/key 密码通常为 `android`；只有确认用的正是这份默认 debug keystore 时才使用这些默认值。自有发布 keystore 请填创建时的 alias 和密码。请勿删除或重建正在使用的 keystore。

配置完成后，推送形如 `v0.4.50` 的版本 tag，Release workflow 会先构建并验收 Android APK，再构建 Windows 安装包，并在同一 Release 发布两个平台的产物。当前开发主机没有 Java/Android SDK，因此需由 GitHub Actions 完成 APK 构建；在四个签名 Secret 配齐前，不会产出可安装的发布 APK。

Windows 与 Android 使用相同的 GitHub 云同步协议。手机第一次使用云同步时，要在“设置 → 云同步”填写同一私有仓库、分支、仓库内目录和 Fine-grained Token；Token 留在手机自己的系统安全存储，任务等业务内容在仓库中仍保持加密。
