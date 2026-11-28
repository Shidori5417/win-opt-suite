# Güvenlik Politikası

WinOptSuite yönetici hakları ile sistem ayarlarını (kayıt defteri, servisler, zamanlanmış görevler, güç planı, güvenlik duvarı) değiştirir. Bu yüzden güvenlik açığı bildirimlerini ciddiye alıyoruz.

## Açık bildirme

Lütfen güvenlik açıklarını **herkese açık issue veya pull request olarak açmayın.**

1. GitHub'ın özel bildirimi: depo sayfasında **Security → Report a vulnerability**.
2. Bu düğme görünmüyorsa, ayrıntı yazmadan "Güvenlik iletişimi talebi" başlıklı bir issue açın; özel bir kanal üzerinden iletişime geçilir.

Bildirimde şunlar bulunsun: ne bulduğunuz, nasıl yeniden üretileceği, etkilenen sürüm veya commit ve beklediğiniz etki.

## Kapsam

Kapsam içi:

- Geri alma/kurtarma mantığının (SQLite WAL durum deposu, işlem yöneticisi) bütünlüğünü bozan hatalar.
- Yetki yükseltme, `manifests/tweaks/*.toml` doğrulamasını (plasebo ve Windows Defender'ı zayıflatma yasağı) aşma.
- Yönetici olarak çalışan süreçte güvenli olmayan dosya/kayıt defteri/yol işleme.

Kapsam dışı: yerel yönetici hakkına zaten sahip bir saldırganın gerektirdiği senaryolar ve bağımlılıklardaki henüz yamalanmamış üçüncü taraf açıkları (ilgili projeye bildirin).

## Desteklenen sürümler

Yalnızca en son sürüm düzeltme alır.
