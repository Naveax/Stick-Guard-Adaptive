# Stick Guard Adaptive

**GuardAdaptive** için kontrolcü *stick drift* ölçüm ve araştırma deposu.

> **Güncel durum (2026-10-16):** Bu depodaki ilk yayın, **work-pc üzerindeki Remote Lab / tarayıcı tabanlı test sitesidir**. Naveax bilgisayarındaki **Rust GuardAdaptive v0.84.0 çekirdeği artık guardadaptive-core/ klasöründedir.** Rust çekirdeği ve work-pc web sitesi henüz entegre değil; gerçek oyun girdisi düzeltmesi üretimde kapalıdır.

## İşlevler

- Tek ekranlı başlangıç: **Stick driftsiz / Stick driftli / Ağır stick driftli** kontrolcü.
- Gamepad API ile sol/sağ analogların canlı koordinat ve yüzde görselleştirmesi.
- **5 saniyelik boşta sapma ölçümü** (her analog için merkeze uzaklığın RMS yüzdesi).
- Deneysel, tarayıcı içi **radyal deadzone simülasyonu** (varsayılan %8, aralık %0–25). Ham ile filtrelenmiş sonuç ayrı gösterilir. **Gerçek kontrolcü/oyun girdisini değiştirmez.**
- Altı fazlı kontrollü ölçüm: sol/sağ analogu serbest bırakma, küçük hareket ve tam tur. Test edilen Gamepad API eksenleri, tuş değerleri, zaman damgaları ve türetilmiş filtre çıktıları örneklenir.
- Tam test sonunda **isteğe bağlı anket ve yorum**; kayıt için **ayrı açık kullanıcı onayı**.
- Sunucu tarafında katı şema denetimi, filtre sonuçlarının yeniden hesaplanarak doğrulanması, 30 günlük saklama hedefi ve yönetici yetkilendirmesi.
- Eski `GA-RL-01` ve `GA-RL-02` karşılaştırma uç noktaları geriye uyumlu tutulur.

## Dizin yapısı

- `remote-lab/public/`: üç seçenekli arayüz, analog görselleştirmeleri ve yönetici görünümleri.
- `remote-lab/server.cjs`: yalnızca `127.0.0.1` üzerinde dinleyen Node.js API sunucusu.
- `remote-lab/telemetry.cjs`: Gamepad API ölçüm şeması, deneme filtresi ve sunucu doğrulaması.
- `remote-lab/paired-study.cjs`: önce/sonra araştırma protokolü.
- `remote-lab/*.test.cjs` ve `remote-lab/test.cjs`: Node.js yerleşik testleri.
- `docs/NAVEAX_SYNC_PLAN.md`: Naveax PC açılınca yapılacak, kaynak sürümü doğrulanarak uygulanacak entegrasyon.

## Çalıştırma

Node.js 22+ önerilir. Çalışma sırasında tüm yapılandırmayı ortam değişkenlerinden ver:

```powershell
cd remote-lab
$env:REMOTE_LAB_PORT = '48930'
$env:REMOTE_LAB_UPLOADS = 'disabled'
node server.cjs
```

Bu şekilde yalnızca yerelde test yapılır; veri yüklemesi kapalıdır. Veri kabulünü açmak için **özel yönetici token dosyası**, veri klasörü, HTTPS üzerinden yayınlanan kesin origin adresi ve saklama/aydınlatma süreçleri gereklidir. Token değerini veya katılımcı verisini hiçbir zaman GitHub'a koymayın.

Testler:

```powershell
cd remote-lab
node --test test.cjs paired-study.test.cjs telemetry.test.cjs
```

## Ölçümün anlamı

Canlı `√(X² + Y²) × 100` değeri analog konumunun merkeze uzaklığıdır. **Drift oranı**, analog serbestken alınan örneklerden hesaplanan RMS değeridir. Filtre sonrası daha düşük yüzde, niyet edilen küçük hareketlerin korunacağını veya oyunda gerçek düzeltme yapıldığını **kanıtlamaz**. Tarayıcı cihazın raporlamadığı fiziksel hareketleri toplayamaz.

## Güvenlik ve gizlilik

Katılımcı örnekleri, ankette yazılan kişisel bilgiler, IP adresleri, cihaz kimlikleri, oturum anahtarları, yönetici tokenları ve `.env` dosyaları **depo dışında** tutulmalıdır. Mevcut araştırma çıktıları **üretim kullanımı için onaylanmamıştır (BLOCKED)** ve otomatik model eğitimine katılmaz. Herkese açık denemelerde yeterli bilgilendirme ve katılımcı rızası gerekir.

**Durum:** Work-pc Remote Lab yayında; Naveax Rust çekirdeği depoda, entegrasyon ve fiziksel kabul bekleniyor.
