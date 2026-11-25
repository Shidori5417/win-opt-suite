# ADR-0003: Tauri v2 masaüstü kabuğu ve otomatik IPC tip üretimi

- **Durum:** Kabul edildi
- **Tarih:** 2026-11-25
- **Karar sahibi:** `opus-5.5`

## Bağlam

`apps/desktop` ince bir kabuk olmalı; Rust ↔ TypeScript tipleri elle
kopyalanmamalı (AGENTS.md §2). crates.io'da Tauri 3 alpha yayınlandı, ancak
proje planı kararlı Tauri v2'yi hedefler.

## Karar

- **Sürümler (sabitlenmiş):** `tauri` 2.12, `tauri-build` 2,
  `tauri-specta` `=2.0.0-rc.29`, `specta` `=2.0.0-rc.28`,
  `specta-typescript` 0.0.15. Tauri 3'e geçiş ayrı bir ADR ister.
- `shared-types` opsiyonel `specta` özelliği taşır; IPC'de görünen her tip
  `#[cfg_attr(feature = "specta", derive(specta::Type))]` ile işaretlidir.
  Varsayılan derlemede `specta` bağımlılığı yoktur.
- `apps/desktop/src-tauri` (crate adı `desktop`) workspace üyesidir;
  `core-engine`, `providers`, `system-probe` ve `shared-types`'ı (specta
  özelliğiyle) bir araya getiren **kompozisyon köküdür**. İş mantığı
  barındırmaz; IPC komutları yalnızca motor çağrısı yapar.
- TypeScript bağlayıcıları (`apps/desktop/src/bindings.ts`) `tauri-specta`
  ile üretilir: bir `#[test] export_bindings` testi dosyayı yeniden yazar.
  CI, testten sonra `git diff --exit-code apps/desktop/src/bindings.ts`
  ile depodaki dosyanın güncel olduğunu doğrular. Elle düzenleme yasaktır.
- 64-bit tamsayılar (`u64`, `i64`) TS'e `number` olarak aktarılır
  (bellek baytı / ms zaman damgası değerleri 2^53 sınırının altındadır).
- Linux geliştirme/CI: Tauri'nin Linux derlemesi `webkit2gtk-4.1` ister;
  ubuntu CI işi `libwebkit2gtk-4.1-dev` vb. paketleri kurar.
- WebView2 yoksa: uygulama Windows'ta WebView2 kurulum yönlendirmesi
  (Tauri `webviewInstallMode: downloadBootstrapper`) kullanır; kurulamazsa
  kullanıcıya `opti-cli` yolunu gösteren yerel mesaj kutusu çıkar.
- Frontend: React 19 + TypeScript `strict` + Vite + Tailwind CSS; paket
  yöneticisi `pnpm` (lockfile depoda).

## Sonuçlar

- (+) Backend'de bir alan değişince bağlayıcı farkı CI'da yakalanır,
  TS `strict` derlemesi kırılır.
- (−) `specta`/`tauri-specta` rc sürümleri tam sabitlenmiştir; yükseltme
  bilinçli yapılmalıdır.
