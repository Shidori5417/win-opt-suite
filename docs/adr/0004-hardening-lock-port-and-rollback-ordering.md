# ADR-0004: Kilit port'u, Win32 named mutex ve geri alma sıralaması

- **Durum:** Kabul edildi
- **Tarih:** 2026-11-25
- **Karar sahibi:** `opus-5.5`

## Bağlam (Faz 4 denetim bulguları)

1. `FileLock` danışma (advisory) kilididir; masaüstü uygulaması ve
   `opti-cli` farklı `--db` yollarıyla çalışırsa aynı anda iki işlem
   sisteme yazabilir.
2. Eski bir `COMMITTED` işlemin geri alınması, aynı kaynağı daha sonra
   değiştiren yeni bir `COMMITTED` işlemin değişikliğini sessizce ezer.
3. Bazı hata durumları `AppError::Internal` ile raporlanıyordu; UI bunları
   yerelleştiremez.

## Karar

- `TransactionLock` / `LockGuard` port'u `shared-types::lock`'a taşınır.
  `core-engine::FileLock` taşınabilir varsayılan olarak kalır;
  `providers` Windows'ta `Global\WinOptSuite.Transaction` adlı Win32
  named mutex tabanlı `NamedMutexLock` sağlar ve kompozisyon kökleri
  Windows'ta bunu kullanır. Terk edilmiş mutex (`WAIT_ABANDONED`) kilidi
  alır; yarım kalan işlem zaten çökme kurtarma ile yakalanır.
- Geri alma: hedef işlemin `APPLIED` kaynaklarından herhangi birini daha
  yeni bir `COMMITTED` işlem değiştirmişse `rollback` reddedilir
  (`AppError::RollbackSuperseded`). Kullanıcı önce yeni işlemi geri alır
  (LIFO).
- Yeni tipli hatalar: `DuplicateProvider`, `InvalidTransactionState`,
  `RollbackSuperseded`.
- **ACL politikası:** Provider'lar hiçbir kaynağın ACL'ini değiştirmez ve
  sahipliğini almaz (TrustedInstaller korumalı anahtarlara yazmaya
  çalışılmaz; `AccessDenied` ile işlem geri alınır). Bu nedenle SDDL
  yedeği gerekmez; `StateSnapshot::security_descriptor_sddl` ileride ACL
  değiştiren bir provider eklenirse zorunlu hale gelir (yeni ADR).

## Sonuçlar

- (+) Süreçler arası gerçek dışlama; sessiz veri ezme yok.
- (−) Kullanıcı eski işlemleri yalnızca ters sırayla geri alabilir.
