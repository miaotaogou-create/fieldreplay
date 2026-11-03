# 自解压启动器（G2）

极薄 Windows 启动器：内嵌与绿色包等价的 zip，首次（或版本变更）展开后拉起 `fieldreplay.exe`。

- 打包入口：`tools\pack_sfx.bat`（需已有 `pack\FieldReplay\`，或加 `-PackGreen`）
- 产物：`pack\FieldReplay.exe`
- 解压优先：启动器同级 `FieldReplay\`；不可写则 `%LOCALAPPDATA%\FieldReplay\`
- 规格：`docs/requirements/20261103-single-exe-sfx.md`
