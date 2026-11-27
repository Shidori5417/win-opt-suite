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

## Platform ve gereksinimler

- **Çalışma:** Windows 11 (x64) ve **yönetici hakları**. Native sağlayıcılar (kayıt defteri, servis, zamanlanmış görev, güç planı, güvenlik duvarı) ve sistem probu yalnızca Windows'ta çalışır. Masaüstü uygulaması WebView2 gerektirir (Windows 11'de hazır gelir; yoksa yükleyici indirir).
- **Linux/macOS:** Yalnızca geliştirme ve test içindir; platformdan bağımsız çekirdek derlenir ve test edilir. `opti-cli` Linux'ta `validate`, `history` ve `drift` komutlarını çalıştırır; `scan`, `dry-run`, `apply` ve `doctor` Windows 11 olmadığı için hata verir (tasarım gereği).
- **Geliştirme araçları:** Rust **1.100.0** (`rust-toolchain.toml` bu sürümü sabitler; `rustup` otomatik kurar) ve masaüstü arayüzü için Node.js 24 + pnpm 10.

## Kurulum

Henüz hazır (önceden derlenmiş) bir sürüm dosyası yayınlanmıyor; kaynaktan derleyin. Windows 11'de, depo kökünde:

```powershell
# Komut satırı aracı -> target\release\opti-cli.exe
cargo build --release -p cli

# Masaüstü uygulaması (Tauri v2; MSI/NSIS çıktısı target\release\bundle altında)
pnpm --dir apps/desktop install --frozen-lockfile
pnpm --dir apps/desktop tauri build
```

`opti-cli`, tweak manifestlerini varsayılan olarak çalıştırılabilir dosyanın yanındaki `manifests/tweaks` dizininden, bulamazsa geçerli dizindeki `./manifests/tweaks` dizininden okur. Başka bir yerden çalıştırırken `--manifests <dizin>` verin.

## Kullanım (yönetici olarak)

```powershell
opti-cli validate                  # manifestleri doğrula, kategori/profil sayılarını yaz
opti-cli scan                      # donanım, OS build, ortam etiketleri, anti-cheat sürücüleri (salt-okunur)
opti-cli doctor                    # platform, veritabanı, katalog ve sağlayıcı tanılama
opti-cli dry-run --profile safe    # sisteme dokunmadan eski -> yeni farkı ve çakışmaları göster
opti-cli apply --profile safe      # atomik, geri alınabilir işlem olarak uygula (y/N onayı; --yes ile atla)
opti-cli apply --tweak privacy.disable-advertising-id
opti-cli history                   # işlem geçmişi (en yeni önce)
opti-cli rollback <işlem-id>       # işlemi ters sırayla geri al
opti-cli recover [işlem-id]        # yarım kalan işlemleri listele / kurtar
opti-cli drift                     # canlı durumu son onaylı taban çizgisiyle karşılaştır
```

- **Profiller:** `safe`, `privacy`, `gaming`, `laptop` (`--profile`). Bir profil yerine bir veya daha fazla tweak kimliği için `--tweak <id>...` kullanın; ikisi birlikte verilemez.
- **Genel bayraklar:** `--json` (makine okunur çıktı), `--db <yol>` (SQLite durum veritabanı; varsayılan `%ProgramData%\WinOptSuite\state.db`), `--manifests <dizin>`.
- **Çıkış kodları:** `0` başarılı, `2` çakışma/engel nedeniyle reddedildi (ör. sanal makinede oyun tweak'leri), `1` diğer hatalar.

## Yapı

| Yol | İçerik |
|---|---|
| `crates/shared-types` | Sözleşmeler (modeller, port'lar, hatalar) |
| `crates/core-engine` | İşlem yöneticisi, dry-run, çakışma, drift, katalog |
| `crates/providers` | Native Windows adaptörleri (tek `unsafe` crate) |
| `crates/system-probe` | Salt-okunur donanım/OS tespiti |
| `crates/cli` | `opti-cli` |
| `apps/desktop` | Tauri v2 masaüstü uygulaması |
| `manifests/` | Tweak manifestleri (`tweaks/*.toml`) ve JSON Schema (`schema.json`); bkz. [manifests/README.md](manifests/README.md) |
| `migrations/` | SQLite durum deposu migrasyonları (yayımlandıktan sonra değiştirilemez) |
| `docs/adr` | Mimari karar kayıtları |

## Geliştirme

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir apps/desktop typecheck && pnpm --dir apps/desktop build
```

Kurallar için `AGENTS.md`, mimari için `docs/adr/` dosyalarına bakın. Güvenlik bildirimleri için [SECURITY.md](SECURITY.md).

## Lisans

[MIT](LICENSE) © 2026 Shidori5417
