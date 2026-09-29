// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createRoot, type Root } from "react-dom/client";
import * as autostart from "@tauri-apps/plugin-autostart";
import { WindowSettings } from "./WindowSettings";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
vi.mock("@tauri-apps/plugin-autostart", () => ({
  isEnabled: vi.fn(),
  enable: vi.fn(),
  disable: vi.fn(),
}));
vi.mock("../lib/window-ipc", () => ({
  windowGetConfig: vi.fn().mockRejectedValue(new Error("窗口配置读取失败")),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
let root: Root | undefined;
afterEach(() => {
  act(() => root?.unmount());
  root = undefined;
  vi.restoreAllMocks();
  document.body.innerHTML = "";
});
async function mount() {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<WindowSettings />));
}
function toggle() {
  const input = document.querySelector(
    '[aria-label="开机自动启动"]',
  ) as HTMLInputElement;
  expect(input).toBeTruthy();
  return input;
}
describe("开机自启设置", () => {
  it("设置入口在窗口配置加载期间也可见", () => {
    expect(renderToStaticMarkup(<WindowSettings />)).toContain("开机自动启动");
  });
  it("读取系统状态，不自动启用；开关操作后再次读取确认", async () => {
    const read = vi
      .spyOn(autostart, "isEnabled")
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(false);
    const enable = vi.spyOn(autostart, "enable").mockResolvedValue();
    const disable = vi.spyOn(autostart, "disable").mockResolvedValue();
    await mount();
    expect(toggle().checked).toBe(false);
    expect(enable).not.toHaveBeenCalled();
    await act(async () => toggle().click());
    expect(enable).toHaveBeenCalledOnce();
    expect(toggle().checked).toBe(true);
    await act(async () => toggle().click());
    expect(disable).toHaveBeenCalledOnce();
    expect(toggle().checked).toBe(false);
    expect(read).toHaveBeenCalledTimes(3);
  });
  it("读取失败时禁用开关，允许重试读取", async () => {
    vi.spyOn(autostart, "isEnabled")
      .mockRejectedValueOnce(new Error("注册表不可读"))
      .mockResolvedValueOnce(true);
    await mount();
    expect(toggle().disabled).toBe(true);
    expect(document.body.textContent).toContain("注册表不可读");
    await act(async () =>
      (
        document.querySelector(
          '[aria-label="重新读取开机自启状态"]',
        ) as HTMLButtonElement
      ).click(),
    );
    expect(toggle().disabled).toBe(false);
    expect(toggle().checked).toBe(true);
  });
  it("启用失败时呈现错误，重新读取真实状态", async () => {
    vi.spyOn(autostart, "isEnabled").mockResolvedValue(false);
    vi.spyOn(autostart, "enable").mockRejectedValue(new Error("拒绝访问"));
    await mount();
    await act(async () => toggle().click());
    expect(toggle().checked).toBe(false);
    expect(document.body.textContent).toContain("拒绝访问");
  });
  it("操作返回成功但系统状态未变时，不显示伪成功", async () => {
    vi.spyOn(autostart, "isEnabled").mockResolvedValue(false);
    vi.spyOn(autostart, "enable").mockResolvedValue();
    await mount();
    await act(async () => toggle().click());
    expect(toggle().checked).toBe(false);
    expect(document.body.textContent).toContain("系统未确认开机自启变更");
  });
  it("关闭失败且无法读取状态时，明确显示未知并禁用开关", async () => {
    vi.spyOn(autostart, "isEnabled")
      .mockResolvedValueOnce(true)
      .mockRejectedValueOnce(new Error("读取失败"));
    vi.spyOn(autostart, "disable").mockRejectedValue(new Error("删除失败"));
    await mount();
    await act(async () => toggle().click());
    expect(toggle().disabled).toBe(true);
    expect(document.body.textContent).toContain("状态未知");
    expect(document.body.textContent).toContain("删除失败");
    expect(document.body.textContent).toContain("读取失败");
  });
  it("系统操作未结束时锁定开关，避免连续提交", async () => {
    vi.spyOn(autostart, "isEnabled")
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);
    let finish!: () => void;
    vi.spyOn(autostart, "enable").mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    await mount();
    await act(async () => toggle().click());
    expect(toggle().disabled).toBe(true);
    expect(toggle().checked).toBe(false);
    await act(async () => finish());
    expect(toggle().checked).toBe(true);
    expect(toggle().disabled).toBe(false);
  });
});
