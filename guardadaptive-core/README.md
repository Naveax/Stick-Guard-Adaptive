# GuardAdaptive Rust çekirdeği (Naveax v0.84.0)

Naveax bilgisayarındaki orijinal çalışma ağacından **birebir** kopyalanmış Rust çekirdeği.

- Kaynak: Naveax, `GuardAdaptive` yerel `main` commit `7f48b73ecd21a3954617f09efe429b5675b48c76` (15 Ekim 2026).
- Cargo sürümü: `0.84.0`; 70 `.rs` modülü, 1 HTML arayüz şablonu, Cargo.toml ve Cargo.lock.
- 16 Ekim 2026: orijinal ve ayrı GitHub yayın kopyasında `cargo test --locked --offline --quiet`: **352 / 352 PASS**.
- Orijinal çalışma ağacı temizdi, dosyalar SHA-256 karşılaştırmasıyla eşlendi.
- Bu kopyaya yüksek riskli yerel betikler, katılımcı ölçümleri, raporlar, makine kimlikleri veya şifreler eklenmedi.
- `remote-lab/` work-pc'de çalışan **ayrı** araştırma sitesi; Rust algoritması bu siteye **henüz entegre değil**.
- **Production gate: BLOCKED**; fiziksel niyet, güvenli sürücü bağlama, mikro hareket kaybı ve gerçek gecikme kabulü tamamlanmadı.

## Test

```bash
cd guardadaptive-core
cargo test --locked --offline
cargo check --locked --offline
```

**Lisans:** Kaynak projede açık kaynak lisans dosyası yoktu. Depo herkese açık olsa da ayrıca bir lisans verilmiş olduğu varsayılmamalıdır.
