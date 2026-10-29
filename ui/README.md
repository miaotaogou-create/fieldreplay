# FieldReplay 界面预览

本目录是 FieldReplay 的 React 界面壳（Vite + Tailwind），用于本地预览录制 / 重演 / 报文 / 对接说明四页。当前为 mock 数据，尚未对接原生录放引擎。

## 本地运行

前提：已安装 Node.js。

```bat
cd ui
npm install
npm run dev
```

浏览器打开终端提示的地址（默认 `http://localhost:3000`）。

## 说明

- 无需 API Key 或云端密钥；纯本地静态预览。
- 产品目标与验收见仓库根目录 `docs/requirements/`、`PROGRESS.md`。
- 引擎与 `.fcap` 落盘在界面阶段完成后再做，见进度本阶段 E。
