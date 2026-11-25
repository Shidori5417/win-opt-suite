# ADR-0002: Salt-okunur `ProbeSource` port'u

- **Durum:** Kabul edildi
- **Tarih:** 2026-11-25
- **Karar sahibi:** `opus-5.5`

## Bağlam

`system-probe` donanım/OS bilgisini Win32 API'leriyle okumalıdır; bu çağrılar
`unsafe` gerektirir. ADR-0001 ve AGENTS.md `unsafe` kodu yalnızca
`crates/providers` içinde izin verir ve `system-probe` yalnızca
`shared-types`'a bağımlı olabilir.

## Karar

- `shared-types::probe::ProbeSource` adlı salt-okunur bir port tanımlanır
  (OS sürümü, CPU/GPU, bellek, güç, şasi tipi, VM, domain, yüklü sürücüler,
  servis adları, çalışan servis/süreç sayısı).
- `system-probe` yalnızca bu port üzerinden çalışır; `SystemFacts` üretimi,
  laptop/anti-cheat tespiti ve metrik toplama saf mantıktır ve Linux CI'da
  sahte (fake) kaynakla test edilir.
- Windows uygulaması `WindowsProbeSource`, `crates/providers/src/win/probe.rs`
  içinde yer alır (`unsafe` + `// SAFETY:`). Bu dosya `hydrogen` ajanının
  yetki alanına verilmiştir; `providers` içindeki diğer dosyalar
  `claude-sonnet`'tadır.
- Kompozisyon kökü (`cli`, `apps/desktop`) `WindowsProbeSource`'u
  `system-probe`'a enjekte eder.

## Sonuçlar

- (+) `unsafe` izolasyonu ve tek yönlü DAG korunur.
- (+) Tespit mantığı platformdan bağımsız test edilir.
- (−) `providers` crate'i salt-okunur bir probe uygulaması da barındırır;
  `SystemProvider` yazma yolundan ayrı modülde tutulur.
