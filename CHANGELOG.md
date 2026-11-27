# Değişiklik Günlüğü

Biçim [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) ve
[Semantic Versioning](https://semver.org/lang/tr/) esas alınır.

## [0.1.0] — 2026-11-25

İlk yayın: native, şeffaf, donanıma duyarlı ve atomik olarak geri
alınabilir Windows 11 optimizasyon paketi.

### Eklendi
- **Sözleşmeler (`shared-types`):** `SystemProvider` port'u, `ActionSpec`,
  `TweakManifest` + doğrulama, `StateSnapshot`, `DryRunDiff`,
  `ConflictWarning`, `DriftReport`, `ProbeSource`, `TransactionLock`,
  tipli `AppError`; Defender/çekirdek servis ve plasebo ayar yasağı veri
  olarak kodlandı; opsiyonel `specta` tip üretimi (ADR-0001..0004).
- **Çekirdek motor (`core-engine`):** SQLite WAL durum deposu ve SHA-256
  doğrulamalı numaralı migrasyonlar; `ProviderRegistry`; `ConflictGuard`;
  salt-okunur `DryRunPlanner`; Saga tabanlı `TransactionManager` (adım
  öncesi kalıcı anlık görüntü, ters sıralı telafi, açılışta çökme
  kurtarma); `DriftDetector`; doğrulanmış tweak `Catalog`'u.
- **Native sağlayıcılar (`providers`):** `windows-rs` ile Registry,
  Service, Scheduled Task (COM), Power Plan ve Firewall (COM)
  adaptörleri; `Global\WinOptSuite.Transaction` named mutex kilidi;
  salt-okunur `WindowsProbeSource`. Tüm `unsafe` bu crate'te ve her biri
  `// SAFETY:` gerekçeli. Harici kabuk süreci yok.
- **Sistem probu (`system-probe`):** OS build, CPU/GPU/RAM, laptop/pil,
  VM (firmware dizgileri), domain, anti-cheat sürücüleri (Vanguard,
  FACEIT, EasyAntiCheat, BattlEye), Önce/Sonra metrikleri.
- **Tweak kataloğu:** 20 GPO öncelikli TOML manifesti, `safe`, `privacy`,
  `gaming`, `laptop` profilleri; `manifests/schema.json` (JSON Schema).
- **`opti-cli`:** `validate`, `scan`, `dry-run`, `apply`, `rollback`,
  `recover`, `history`, `drift`, `doctor`; `--json` çıktı.
- **Masaüstü uygulaması (`apps/desktop`):** Tauri v2 + React 19 + strict
  TypeScript; `tauri-specta` ile otomatik IPC tipleri; sistem özeti,
  tweak kataloğu, dry-run diff, geri alma geçmişi ve Önce/Sonra raporu;
  TR/EN arayüz.
- **CI:** Windows + Linux fmt/clippy/test, Windows'ta salt-okunur
  `opti-cli` smoke testi, politika denetimleri (harici kabuk, `unsafe`
  yeri, değişmez migrasyonlar), manifest şema doğrulaması, `cargo-deny`.

### Güvenlik
- Gömülü SQLite, RUSTSEC-2026-0459 (CVE-2026-119375) için yamalı sürüme
  yükseltildi (`rusqlite` 0.43).
- Daha yeni bir işlemin değiştirdiği kaynağı ezen geri almalar reddedilir
  (`RollbackSuperseded`).
- Windows 11'de VBS nedeniyle CPUID hypervisor bitinin fiziksel
  makinelerde de set olması kaynaklı yanlış VM tespiti düzeltildi.

### Bilinen sınırlamalar
- Var olmayan bir zamanlanmış görev/servis hedefleyen tweak `apply`
  sırasında `NotFound` ile tüm işlemi geri alır (opsiyonel kaynak
  desteği planlanıyor).
- Registry geri alması yalnızca değeri siler; `apply` sırasında
  oluşturulan ara anahtarlar kalır.
- Masaüstü uygulaması ve native sağlayıcılar yalnızca Windows'ta
  çalışır; Linux'ta derlenir ve `unsupported_platform` hatası gösterir.

[0.1.0]: https://github.com/Shidori5417/win-opt-suite/releases/tag/v0.1.0
