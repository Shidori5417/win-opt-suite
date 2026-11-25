# ADR-0001: Hexagonal mimari, atomik geri alma ve sözleşme öncelikli geliştirme

- **Durum:** Kabul edildi
- **Tarih:** 2026-11-25
- **Karar sahibi:** `opus-5.5` (Baş Mimar)

## Bağlam

Windows 11 için şeffaf, donanıma duyarlı ve atomik olarak geri alınabilir bir
optimizasyon aracı geliştiriyoruz. Proje yıllarca yeni Windows sürümleri,
tweak'ler ve alt sistemlerle büyüyecek; birden çok ajan paralel çalışacak.
Bu yüzden (1) çekirdek mantığın işletim sisteminden bağımsız test
edilebilmesi, (2) yarıda kalan işlemlerin elektrik kesintisinde bile güvenle
geri alınabilmesi ve (3) paralel geliştirmenin dondurulmuş sözleşmelere
dayanması gerekir.

## Karar

### 1. Ports & Adapters, tek yönlü bağımlılık grafiği

```text
            shared-types   (saf modeller, SystemProvider port'u, AppError)
           /     |      \
   core-engine providers system-probe
           \     |      /
          cli   apps/desktop   (ince kabuklar; yalnızca orkestrasyon)
```

- `shared-types` hiçbir iç crate'e ve platform API'sine bağlı değildir.
- `core-engine` yalnızca `shared-types`'a bağlıdır ve OS ile **yalnızca**
  `dyn SystemProvider` üzerinden konuşur → Linux CI'da dahi %100 test edilir.
- `providers` tek `unsafe` barındırabilen crate'tir (`windows-rs`);
  diğer tüm crate'ler `#![forbid(unsafe_code)]` taşır.
- `system-probe` salt-okunurdur.
- Kabuklar (`cli`, `apps/desktop`) iş mantığı içermez; `core-engine`,
  `providers` ve `system-probe`'u bir araya getirir (composition root).

### 2. `SystemProvider` sözleşmesi ve `ProviderRegistry`

`read / apply / revert / verify` metotları (bkz.
`crates/shared-types/src/provider.rs`). `TransactionManager` somut provider
tanımaz; `ProviderKind` → `Box<dyn SystemProvider>` eşlemesi yapan
`ProviderRegistry` üzerinden çalışır. Yeni bir alt sistem eklemek =
`ProviderKind` + `ActionSpec` varyantı (sözleşme değişikliği, ADR ister) +
yeni provider + kayıt. `TransactionManager` değişmez.

`revert(action, snapshot)` eylemi de alır, çünkü `existed == false`
olduğunda kaynağın **silinmesi** için konum bilgisine ihtiyaç vardır.
`action_json` bu yüzden veritabanında saklanır.

### 3. Saga tabanlı atomik işlem ve çökme kurtarma

1. Global sistem mutex'i alınır (aynı anda tek işlem).
2. İşlem `PENDING` olarak yazılır; ardından `APPLYING`.
3. Her eylem için: `read()` → anlık görüntü `action_snapshots` tablosuna
   `PENDING` yazılır (WAL, fsync) → `apply()` → `APPLIED`.
4. Hata olursa işlem `ROLLING_BACK` olur ve yalnızca `APPLIED` adımlar
   `seq` sırasının **tersine** `revert()` edilir. `PENDING` adımın provider
   tarafından kısmen yazılmış olabileceği durum için bu adım da (idempotent)
   revert edilir.
5. Tüm adımlar geri alınırsa `ROLLED_BACK`, herhangi biri başarısızsa
   `FAILED` (+ `REVERT_FAILED` adımlar) — kullanıcıya açıkça raporlanır.
6. Başarı: `COMMITTED`.
7. **Startup Crash Recovery:** açılışta `PENDING / APPLYING / ROLLING_BACK`
   işlem varsa kullanıcıya kurtarma (ters sıralı geri alma) akışı sunulur;
   kurtarma bitmeden yeni işlem başlatılamaz (`AppError::RecoveryRequired`).

### 4. Versiyonlama

- **SQLite:** numaralı, yalnızca ileri yönlü migrasyonlar
  (`migrations/NNNN_<konu>.sql`), `PRAGMA user_version` + `schema_migrations`
  tablosunda SHA-256 sağlama toplamı. Yayınlanmış dosya değişirse
  yürütücü hata verir. Geri dönüş ihtiyacı yeni bir ileri migrasyonla
  karşılanır (down-migration yok: veri kaybı riski).
- **Manifest:** her TOML `schema_version = 1` taşır; `serde(deny_unknown_fields)`
  ile yazım hataları reddedilir; `TweakManifest::validate()` + (Faz 2)
  `manifests/schema.json` CI'da doğrulanır.
- **IPC:** Rust tipleri `tauri-specta` ile TypeScript'e üretilir; elle tip
  kopyalanmaz.

### 5. Güvenlik politikası veri olarak kodlanır

`shared_types::policy` Defender/çekirdek güvenlik servislerine ve bilinen
plasebo değerlere (`NetworkThrottlingIndex`, `SystemResponsiveness`)
dokunan eylemleri manifest doğrulamasında reddeder. Harici kabuk süreçleri
(`powershell.exe`, `cmd.exe`, `reg.exe`, `sc.exe`) yasaktır.

### 6. Hata modeli

Üretim kodunda panik yok (`unwrap/expect/panic/todo/unimplemented`
workspace lint'leriyle `deny`). Tüm hatalar `thiserror` + `serde` ile tipli
`AppError` olarak taşınır; IPC'de `code` etiketiyle serileşir ve UI bunu
yerelleştirir. Kod, tanımlayıcılar ve hata metinleri İngilizce; proje
dokümantasyonu Türkçedir.

## Onaylanmış bağımlılıklar

Aşağıdakiler bu ADR ile onaylıdır; listede olmayan her yeni bağımlılık yeni
bir ADR gerektirir.

| Crate | Kullanım yeri | Gerekçe |
|---|---|---|
| `serde`, `serde_json`, `thiserror` | tümü | Serileştirme, tipli hata |
| `toml` | manifest yükleme, testler | Bildirimsel tweak'ler |
| `rusqlite` (`bundled`) | `core-engine` | Tek dosya WAL durumu; sistem SQLite'ına bağımlılık yok |
| `sha2` | `core-engine` | Migrasyon sağlama toplamı |
| `uuid` (`v7`) | `core-engine` | Zaman sıralı işlem kimliği |
| `windows` (`cfg(windows)`) | `providers`, `system-probe` | Native Win32/COM; harici kabuk yok |
| `clap` (`derive`) | `cli` | Komut satırı ayrıştırma |
| `tauri` v2, `tauri-specta`, `specta` | `apps/desktop` | Masaüstü kabuk + otomatik TS tipleri |

## Sonuçlar

- (+) Çekirdek motor ve sözleşmeler her platformda test edilebilir;
  CI Linux + Windows matrisiyle sınırları zorlar.
- (+) Yeni provider eklemek çekirdeği değiştirmez.
- (−) `ActionSpec`/`ProviderKind` kapalı enum'dur; yeni alt sistem bir
  sözleşme değişikliği gerektirir (bilinçli tercih: tip güvenliği ve
  şema doğrulaması için).
