# Tweak Manifestleri

Bu dizin uygulamanın tüm optimizasyonlarını bildirimsel olarak tanımlar.
Her tweak tek bir dosyadır: `manifests/tweaks/<id>.toml`. Dosya adı, dosyadaki
`id` alanıyla birebir aynı olmalıdır (ör. `interface.disable-widgets` →
`interface.disable-widgets.toml`).

Şema: `manifests/schema.json` (JSON Schema draft 2020-12). Rust tarafındaki
karşılığı `crates/shared-types/src/manifest.rs` ve `action.rs` dosyalarıdır.
Şema ile Rust doğrulaması birlikte çalışır; biri diğerinin yerini tutmaz.

## Yeni tweak ekleme kontrol listesi

AGENTS.md §3 "Yeni Tweak Eklerken" maddeleri:

- [ ] Yalnızca TOML dosyası eklendi; Rust koduna sabit (hardcoded) bir ayar yazılmadı.
- [ ] Mümkünse kullanıcı anahtarı yerine GPO politika yolu (`HKLM\SOFTWARE\Policies\...`) kullanıldı.
- [ ] Plasebo bir ayar değildir; Windows Defender'ı veya temel güvenlik servislerini bozan bir ayar içermez.
- [ ] `min_windows_build`, `excluded_on`, `conflicts_with` ve açık `side_effects` açıklaması dolduruldu.
- [ ] Şema doğrulaması ve katalog testi geçti (aşağıdaki "Yerelde doğrulama" bölümü). CLI hazır olduğunda `opti-cli dry-run` ile kuru çalıştırma da yapılır.

Adımlar:

1. `manifests/tweaks/` altına `<id>.toml` dosyasını oluşturun. Kimlik kuralı: küçük harf,
   rakam ve `.`/`-` ayırıcılar; 3–64 karakter; ardışık veya baştaki/sondaki ayırıcı yok.
2. `schema_version = 1` yazın.
3. Yukarıdaki kontrol listesini tamamlayın.
4. Doğrulamayı çalıştırın (aşağıya bakın).
5. Yeni bir tweak için Rust tarafında kod değişikliği gerekmez. Yeni bir sağlayıcı türü
   (provider) gerekirse bu bir ADR konusudur (`docs/adr/`).

## Alan referansı

| Alan | Zorunlu | Açıklama |
|---|---|---|
| `schema_version` | evet | Her zaman `1`. |
| `id` | evet | Tweak kimliği, dosya adıyla aynı. |
| `title` | evet | Kısa, insan tarafından okunabilir ad. |
| `description` | evet | Tweak'in ne yaptığı. |
| `category` | evet | `privacy`, `telemetry`, `performance`, `gaming`, `power`, `network`, `interface`, `services`, `updates`, `security`. |
| `risk` | evet | `low`, `medium`, `high`. |
| `side_effects` | evet | Kullanıcının göreceği yan etkiler. Dürüst ve açık olmalı. Boş bırakılamaz. |
| `min_windows_build` | evet | En düşük Windows build numarası. Windows 11 için en az `22000`. |
| `max_windows_build` | hayır | En yüksek build. `min_windows_build` değerinden küçük olamaz (Rust denetler). |
| `excluded_on` | hayır | Bu ortamlarda tweak uygulanmaz: `laptop`, `desktop`, `on_battery`, `virtual_machine`, `domain_joined`, `anti_cheat_vanguard`, `anti_cheat_faceit`, `anti_cheat_easy_anti_cheat`, `anti_cheat_battl_eye`. |
| `conflicts_with` | hayır | Birlikte seçilemeyecek tweak kimlikleri. Hedef kimlik katalogda bulunmalıdır. |
| `profiles` | hayır | Tweak'in dahil olduğu profiller (aşağıya bakın). |
| `requires_reboot` | hayır | Uygulamadan sonra yeniden başlatma gerekiyorsa `true`. |
| `references` | hayır | `https://` ile başlayan dokümantasyon bağlantıları (tercihen Microsoft Learn). |
| `actions` | evet | En az bir eylem. Her eylem `provider` alanıyla türünü belirtir. |

### Eylem türleri

- **registry**: `provider = "registry"`, `hive` (`HKLM` veya `HKCU`), `key`, `value_name`.
  Ya `value = { type = "...", data = ... }` ya da `delete = true` verilir; ikisi birden olamaz.
  Desteklenen `type` değerleri: `dword` (0–4294967295), `qword`, `string`, `expand_string`,
  `multi_string` (dizi), `binary` (0–255 değerinden oluşan dizi).
- **service**: `provider = "service"`, `name` (servis anahtar adı, görünen ad değil),
  `start_type` (`automatic`, `automatic_delayed`, `manual`, `disabled`).
- **scheduled_task**: `provider = "scheduled_task"`, `path` (tam görev yolu), `enabled`.
- **power_plan**: `op = "set_value"` (`subgroup_guid`, `setting_guid`, `ac_value`, `dc_value`,
  isteğe bağlı `scheme_guid`; yoksa aktif plan) veya `op = "activate_scheme"` (`scheme_guid`).
  GUID biçimi: `8-4-4-4-12` onaltılık, süslü parantezsiz.
- **firewall**: `op = "set_rule_enabled"` (`rule_name`, `enabled`) veya `op = "ensure_rule"`
  (`rule` tablosu). Uygulamanın kendi kuralları `WinOptSuite-` önekini taşır.

Aynı kaynağa (ör. aynı registry değeri) bir tweak içinde iki kez dokunulamaz.

## Profiller

- `safe`: Önerilen, geri dönüşü kolay ve performans/gizlilik açısından güvenli temel ayarlar.
- `privacy`: Telemetri, reklam kimliği, kişiselleştirme gibi gizlilik odaklı ayarlar.
  Bazı geri bildirim ve tanılama özellikleri devre dışı kalabilir.
- `gaming`: Oyun performansına yönelik ayarlar (Game DVR, GPU zamanlaması, USB güç yönetimi).
  Bazıları donanım/sürücü gereksinimi taşır.
- `laptop`: Pil ömrüne yönelik güç ayarları. Dizüstü bilgisayarlarda anlamlıdır.

Bir tweak birden fazla profilde yer alabilir veya hiçbirinde yer almayabilir
(hiçbir profile ait olmayan tweak yalnızca açıkça seçildiğinde uygulanır).

## Yerelde doğrulama

Şema doğrulaması (CI'daki `manifests` işiyle aynı komut):

```bash
python3 -m venv /tmp/cjs
/tmp/cjs/bin/pip install check-jsonschema==0.33.0
/tmp/cjs/bin/check-jsonschema --schemafile manifests/schema.json manifests/tweaks/*.toml
```

Rust tarafındaki katalog doğrulaması (dosya adı = id, profil dolu, profil içi çakışma yok,
iki Delivery Optimization tweak'inin çakıştığı):

```bash
cargo test -p core-engine --test catalog_manifests
```

Tüm kalite kapıları (commit öncesi zorunlu):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
