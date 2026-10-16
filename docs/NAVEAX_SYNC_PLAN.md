# Naveax PC entegrasyon planı (2026-10-16)

**Şu an:** Bu depoda yalnız work-pc Remote Lab'in tarayıcı tabanlı test uygulaması yer alır. Naveax PC kapalı olduğundan asıl Rust GuardAdaptive çekirdeği **burada yoktur**. En son anımsanan sürüm `v0.84.0`, ana Remote Lab `v0.1.6`; bu sürümler dosyadan güncel olarak doğrulanmadı.

## Naveax açıldığında

1. Nexowire bağlantısını doğrula. Önce mevcut `C:\\Users\\navea\\source\\GuardAdaptive` çalışma ağacını, Git branch/HEAD durumunu ve commit edilmemiş değişiklikleri salt okunur incele.
2. Rust algoritmasını, mevcut test sonuçlarını ve sürücü güvenlik sınırlarını doğrula; projeyi sıfırdan kurma veya başka bilgisayardaki kodla körlemesine üzerine yazma.
3. Depo **public**: yönetici tokenları, Cloudflare/Tailscale kimlik bilgileri, katılımcı JSON'ları, kayıtlar, şifreler veya makineye özgü gizli yapılandırmaları kesinlikle paylaşma.
4. Rust çekirdeğini modüler biçimde ayrı bir dizine taşı; filtreyi tarayıcıdaki `radial-demo-v1` ile karıştırma.
5. Aynı ham Gamepad API kayıtlarında mevcut radyal filtre ve gerçek GuardAdaptive'ı ölç: boşta RMS sapma, kasıtlı küçük hareket kaybı, tepki gecikmesi, kararlılık. Simüle iyileşme gerçek cihaz düzeltmesi değildir.
6. Fiziksel kullanıcı kabulü ve güvenlik doğrulaması olmadan üretim sürücüsü veya gerçek oyun input müdahalesi başlatma. Üretim kapısı **BLOCKED**.
7. Güvenlik açısından engellenmiş `scripts/Run-Lab.ps1` ve `scripts/Test-GuardAdaptive-v51.ps1` dosyalarını çalıştırma.
8. Doğrulanmış kodları ayrı commit/PR olarak ekle; work-pc yayını ve mevcut araştırma kayıtları korunmalı.
