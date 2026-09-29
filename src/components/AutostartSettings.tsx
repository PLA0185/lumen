import { useCallback, useEffect, useState } from "react";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";

export function AutostartSettings() {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setEnabled(await isEnabled());
    } catch (e) {
      setEnabled(null);
      setError(`读取开机自启状态失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const change = async (next: boolean) => {
    setBusy(true);
    setError(null);
    try {
      await (next ? enable() : disable());
      const actual = await isEnabled();
      setEnabled(actual);
      if (actual !== next)
        setError("系统未确认开机自启变更，请重新读取状态后重试。");
    } catch (e) {
      let message = `修改开机自启失败：${String(e)}`;
      try {
        setEnabled(await isEnabled());
      } catch (readError) {
        setEnabled(null);
        message += `；读取状态失败：${String(readError)}`;
      }
      setError(message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="setgroup">
      <h3 className="setgroup__title">启动</h3>
      <div className="setrow">
        <div className="setrow__label">
          <div className="setrow__title">开机自动启动</div>
          <div className="setrow__desc">
            开启后，登录当前 Windows 账户时自动启动
            Lumen，继续显示任务和处理提醒。
          </div>
        </div>
        <label className="setrow__control">
          <input
            type="checkbox"
            aria-label="开机自动启动"
            checked={enabled === true}
            disabled={busy || enabled === null}
            onChange={(e) => void change(e.target.checked)}
          />
          <span>
            {busy
              ? "处理中…"
              : enabled === null
                ? "状态未知"
                : enabled
                  ? "已开启"
                  : "已关闭"}
          </span>
        </label>
      </div>
      {error && (
        <p className="alert alert--error selectable" role="alert">
          {error}
        </p>
      )}
      <button
        type="button"
        className="btn btn--ghost btn--sm"
        aria-label="重新读取开机自启状态"
        disabled={busy}
        onClick={() => void reload()}
      >
        重新读取状态
      </button>
    </div>
  );
}
