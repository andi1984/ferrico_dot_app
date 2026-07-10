// Synchronous user-agent sniff — the mobile/desktop split happens at the
// React root, before any async plugin call could resolve. If detection ever
// needs to be authoritative (e.g. a desktop browser spoofing a mobile UA),
// switch to `@tauri-apps/plugin-os`'s `platform()`, which is async and would
// require rendering a loading state before choosing a root component.
export function isMobilePlatform(): boolean {
  return /Android|iPhone|iPad/.test(navigator.userAgent)
}
