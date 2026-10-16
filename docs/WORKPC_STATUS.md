# Work-pc Remote Lab | Durum: 2026-10-16

- Canlı çalışma yeri work-pc; yayın: `https://nexowire.tail10f02d.ts.net:8443/`.
- Üç basit kol seçeneği: driftsiz, driftli, ağır driftli.
- Tarayıcı Gamepad API ile canlı ham ve radyal filtrelenmiş sol/sağ analog göstergesi.
- **Boşta** drift ölçüsü: 5 saniye merkezden uzaklık RMS yüzdesi. Hareket sırasındaki gösterge anlık analog konumudur, drift değildir.
- Tam 6 fazlı çalışma: sol/sağ serbest, küçük hareket, tam tur; yaklaşık 20 FPS ile tarayıcının desteklediği eksenler, tuşlar, zaman damgası ve sanal filtrelenmiş çıktı.
- Test bitince isteğe bağlı anket ve yorum; work-pc'de JSON saklama yalnız ayrı kullanıcı izniyle.
- Veri doğrulama/erişim sınırları: strict şema, yeniden hesaplanan filtre, HTTPS Origin, gizli yönetici tokenı; ayrı oturum kimliğiyle kayıt/silme, 30 gün saklama hedefi.
- İzole Edge sentetik kontrolcü testi: 1440 örnek, anket akışı PASS; bu gerçek kol kaydı değildir.
- Node.js otomatik testler: 11/11 PASS. Gerçek fiziksel kol kabulü henüz yapılmadı.
- Sunucu: yalnız `127.0.0.1:48930`; dış erişim Tailscale Funnel 8443.
- Rust algoritma **henüz bulunmuyor**; radyal filtre yalnız simülasyon, oyun girdilerini etkilemez. Production gate: **BLOCKED**.
- Katılımcı ham verisi, anahtarlar ve gerçek kullanıcı yorumları **GitHub'a yüklenmez**.
