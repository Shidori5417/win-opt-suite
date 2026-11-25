# WinOptSuite

Windows 11 için native, şeffaf, donanıma duyarlı ve **atomik olarak geri
alınabilir** sistem optimizasyon aracı (Rust + Tauri v2).

- Her değişiklikten önce kaynağın durumu SQLite WAL'a yazılır; hata olursa
  uygulanan adımlar ters sırayla geri alınır, elektrik kesintisinden sonra
  açılışta kurtarma yapılır.
- Tweak'ler kodda değil, `manifests/tweaks/*.toml` dosyalarında; mümkün
  olan her yerde GPO (`HKLM\SOFTWARE\Policies\...`) yolları kullanılır.
- Plasebo ayarlar ve Windows Defender'ı zayıflatan değişiklikler doğrulama
  aşamasında reddedilir. PowerShell/cmd çağrısı yoktur; yalnızca
  `windows-rs` native API'leri.

## Yapı

| Yol | İçerik |
|---|---|
| `crates/shared-types` | Sözleşmeler (modeller, port'lar, hatalar) |
| `crates/core-engine` | İşlem yöneticisi, dry-run, çakışma, drift, katalog |
| `crates/providers` | Native Windows adaptörleri (tek `unsafe` crate) |
| `crates/system-probe` | Salt-okunur donanım/OS tespiti |
| `crates/cli` | `opti-cli` |
| `apps/desktop` | Tauri v2 masaüstü uygulaması |
| `docs/adr` | Mimari karar kayıtları |

## Kullanım (yönetici olarak)

```powershell
opti-cli doctor
opti-cli dry-run --profile safe
opti-cli apply --profile safe
opti-cli history
opti-cli rollback <işlem-id>
opti-cli drift
```

## Geliştirme

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir apps/desktop typecheck && pnpm --dir apps/desktop build
```

Kurallar için `AGENTS.md`, mimari için `docs/adr/` dosyalarına bakın.
