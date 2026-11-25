# AGENTS.md — Kalıcı Mimari, Kod Kalitesi ve Git Kuralları

Bu dosya proje kökünde kalıcı olarak yer alır. Projede çalışan **Ana Ajan (`opus-5.5`)** ve tüm yetkili alt ajanlar her oturumda bu kurallara istisnasız uymak zorundadır.

---

## 1. Ajan Hiyerarşisi ve Yetki Sınırları

| Ajan ID | Rol | Yetkili Olduğu Kapsam |
|---|---|---|
| `opus-5.5` | **Ana Ajan & Baş Mimar** | Mimari kararlar, `shared-types`, ADR'ler, kod/güvenlik denetimi, branch/merge/release yönetimi |
| `claude-sonnet` | **Kıdemli Rust & Win32 Mühendisi** | `crates/core-engine`, `crates/providers`, `migrations/` |
| `hydrogen` | **Sistem Probu, Drift & CLI Uzmanı** | `crates/system-probe`, `crates/cli`, `DriftDetector` |
| `neon` | **Tauri v2 & Frontend Uzmanı** | `apps/desktop` (Tauri IPC + React/TS arayüzü) |
| `claude-haiku` | **Paralel Test & Katalog İşçisi** | `manifests/`, mock/birim testleri, i18n, dokümantasyon |

- **Model eşlemesi:** `claude-sonnet` → Claude Sonnet 5.5, `claude-haiku` → Claude Haiku 5.5, `hydrogen` → Gemini 4 Hydrogen, `neon` → Gemini 4 Neon.
- **Kesin Yasak:** Yukarıdaki eşlemede bulunmayan hiçbir ajan veya model kullanılamaz. Genel `gemini-4` modelleri yasaktır; yalnızca `hydrogen` ve `neon` rollerine atanan Gemini 4 Hydrogen ve Gemini 4 Neon varyantlarına kullanıcı kararıyla (2026-11-25) izin verilmiştir.
- **Kapsam istisnası (ADR-0002):** `crates/providers/src/win/probe.rs` ve `crates/providers/src/smbios.rs` dosyaları `hydrogen`'in yetki alanındadır.

---

## 2. Değişmez Mimari Kurallar (Gelecekte Hata Almayı Önleyen Bariyerler)

### Crate Bağımlılık Sınırları (Tek Yönlü DAG)
- `shared-types`: Hiçbir iç crate'e veya `windows-rs`'e bağımlı olamaz.
- `core-engine`: Yalnızca `shared-types`'a bağımlıdır. Win32/COM API'lerini doğrudan import **edemez**.
- `providers`: Yalnızca `shared-types` ve `windows-rs`'e bağımlıdır. Tüm `unsafe` blokları yalnızca bu crate içindedir.
- `system-probe`: Yalnızca `shared-types` ve salt-okunur sistem sorgularına bağımlıdır (sistemi **değiştiremez**).
- `cli` ve `apps/desktop`: Yalnızca orkestrasyon çağrısı yapan ince kabuklardır; iş mantığı veya doğrudan sistem çağrısı barındıramaz.

### Kod ve Bellek Güvenliği
- Üretim kodunda `unwrap()`, `expect()`, `panic!()`, `todo!()`, `unimplemented!()` **yasaktır** (yalnızca `#[cfg(test)]` içinde serbesttir).
- Her `unsafe` bloğunun hemen üstünde `// SAFETY:` açıklaması zorunludur; açıklamasız `unsafe` kabul edilmez.
- `powershell.exe`, `cmd.exe`, `reg.exe`, `sc.exe` gibi harici kabuk süreçleri çağırmak **yasaktır**; yalnızca `windows-rs` native API kullanılır.

### Veritabanı, Manifest ve IPC Değişiklik Kuralları
1. **SQLite Migrasyonları:** `migrations/` altında yayınlanmış hiçbir `.sql` dosyası sonradan düzenlenmez. Her şema değişikliği yeni numaralı dosya (`0002_add_xyz.sql`) olarak eklenir.
2. **Manifest Şeması:** Her `manifests/tweaks/*.toml` dosyası `schema_version` alanı taşımalı ve `manifests/schema.json` doğrulamasından geçmelidir.
3. **IPC Tip Senkronizasyonu:** `apps/desktop` tarafında manuel Rust-to-TS tip kopyalaması yapılmaz; Rust IPC tipleri değiştiğinde `tauri-specta` ile TypeScript bağlayıcıları yeniden üretilir.
4. **Mimari Karar Kaydı (ADR):** Yeni bir bağımlılık, yeni bir provider türü veya veri modelinde kırıcı bir karar alınacaksa `docs/adr/` altına yeni bir ADR kaydı eklenir.

---

## 3. Yeni Özellik Ekleme Kontrol Listeleri

### Yeni Tweak Eklerken
- [ ] Yalnızca TOML dosyası eklendi (Rust koduna hardcoded ayar yazılmadı).
- [ ] Mümkünse kullanıcı anahtarı yerine GPO politika yolu (`HKLM\SOFTWARE\Policies\...`) kullanıldı.
- [ ] Plasebo veya Windows Defender'ı bozan ayar içermiyor.
- [ ] `min_windows_build`, `excluded_on`, `conflicts_with` ve açık yan etki açıklaması dolduruldu.
- [ ] Şema doğrulama testi ve `opti-cli dry-run` testi geçti.

### Yeni SystemProvider Eklerken
- [ ] `SystemProvider` trait'inin tüm metotları (`read`, `apply`, `revert`, `verify`) eksiksiz uygulandı.
- [ ] İşlem öncesi kaynak yoksa (`existed == false`), `revert()` kaynağı sıfırlamak yerine tamamen siliyor.
- [ ] `ProviderRegistry` içine kaydedildi; `TransactionManager` değiştirilmedi.
- [ ] Kısmi hata ve ters sıralı geri alma birim testi eklendi.

---

## 4. Git Disiplini ve Kalite Kapıları

- **Korumalı Dallar:** `main` ve `develop` dallarına doğrudan commit veya `--force` push kesinlikle yasaktır.
- **Branch Adlandırma:** `feat/<konu>`, `fix/<konu>`, `refactor/<konu>`, `chore/<konu>`, `docs/<konu>`.
- **Commit Mesajı:** Conventional Commits (`feat(core): ...`, `fix(providers): ...`, `test(rollback): ...`).
- **Commit Öncesi Zorunlu Komutlar:**
  ```bash
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```
  *(Frontend değişikliği varsa ek olarak `pnpm --dir apps/desktop typecheck && pnpm --dir apps/desktop build`.)*
- **Push Kuralı:** Kalite kapısı yeşil olmadan commit atılmaz; her tamamlanan mantıksal adım commitlenip aktif özelliğin uzak dalına pushlanır. Faz sonlarında `opus-5.5` onayıyla `develop` içine `--no-ff` merge yapılır ve sürüm/faz etiketi pushlanır.