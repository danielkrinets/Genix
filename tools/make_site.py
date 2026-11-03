#!/usr/bin/env python3
"""Builds the static site in site/ (Russian at the root, English in site/en) from the texts below.
Usage: python3 tools/make_site.py. Legal pages contain [PLACEHOLDERS] that the vendor must fill in; see site/README.md."""
import pathlib, re

ROOT = pathlib.Path(__file__).resolve().parent.parent / "site"
PRICE = "$0.99"
REPO = "https://github.com/danielkrinets/Genix/releases/tag/genix-latest"

CSS = """:root{--bg:#f2f2f7;--card:#fff;--text:#1c1c1e;--muted:#63636d;--accent:#0065d4;--line:#dcdce1}
@media(prefers-color-scheme:dark){:root{--bg:#000;--card:#1c1c1e;--text:#f5f5f7;--muted:#aeaeb8;--accent:#5ca6ff;--line:#404044}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--text);font:17px/1.55 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif}
a{color:var(--accent)}header,main,footer{max-width:960px;margin:0 auto;padding:0 20px}
header{display:flex;justify-content:space-between;align-items:center;padding-top:18px;padding-bottom:18px}
.logo{font-weight:700;font-size:20px;color:var(--text);text-decoration:none}nav a{margin-left:18px;text-decoration:none;color:var(--muted);font-size:15px}
.hero{padding:56px 0 36px;text-align:center}h1{font-size:clamp(34px,6vw,56px);line-height:1.1;margin:0 0 16px}
.lead{font-size:20px;color:var(--muted);max-width:640px;margin:0 auto 28px}
.btn{display:inline-block;background:var(--accent);color:#fff;padding:13px 26px;border-radius:14px;text-decoration:none;font-weight:600;margin:4px}
.btn.alt{background:var(--card);color:var(--text);border:1px solid var(--line)}
.shot{width:100%;border-radius:20px;border:1px solid var(--line);margin:28px 0}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(250px,1fr));gap:16px;margin:24px 0}
.card{background:var(--card);border-radius:20px;padding:22px;border:1px solid var(--line)}.card h3{margin:0 0 8px;font-size:18px}.card p{margin:0;color:var(--muted);font-size:15px}
h2{font-size:30px;margin:52px 0 8px}.price{font-size:44px;font-weight:700}.small{font-size:14px;color:var(--muted)}
ul.tick{list-style:none;padding:0}ul.tick li:before{content:"✓  ";color:var(--accent);font-weight:700}
.legal{max-width:720px}.legal h2{font-size:22px;margin:32px 0 6px}.note{background:var(--card);border-left:4px solid #d97a00;padding:12px 16px;border-radius:8px;font-size:15px}
footer{padding-top:40px;padding-bottom:50px;color:var(--muted);font-size:14px}footer a{margin-right:16px}"""

T = {
 "ru": dict(
  lang="ru", prefix="", other="/en/", other_label="English", home="Главная", terms_t="Условия использования", priv_t="Конфиденциальность", ref_t="Возврат средств",
  title="Genix — параметрический CAD для Mac", desc="Нативный параметрический CAD: эскизы, операции, история, экспорт STL, STEP, DXF. 7 дней бесплатно, потом $0.99 в месяц.",
  h1="Параметрический CAD, который быстро открывается и приятно работает", lead="Genix — нативная программа для моделирования деталей: эскиз, выдавливание, отверстия, скругления, массивы, сборочные тела и экспорт в STL, STEP, DXF. Без облака, без регистрации на сервере.",
  dl="Скачать для macOS", buy="Подписка " + PRICE + " в месяц", shots=["Библиотека моделей Genix 0.00.46", "Бокал с трубочкой, собранный в Genix"],
  feats_h="Что умеет", feats=[("Эскизы","Линии, дуги, окружности, эллипс, сплайн, текст, размеры и зависимости, зеркало и массивы."),("Операции","Выдавливание с уклоном и тонкой стенкой, вращение, отверстия, скругление, фаска, оболочка, резьба."),("История","Каждый шаг можно изменить; модель пересчитывается. Параметры и выражения."),("Тела","Объединение, вычитание, пересечение, исключающее, разрез, зеркало, массивы, проверка пересечений."),("Измерения","Расстояния, углы, площади граней."),("Экспорт","STL, 3MF, OBJ, STEP и DXF, открытие в слайсере.")],
  price_h="Цена", price_lead="7 дней бесплатно, потом " + PRICE + " в месяц. Отмена в любой момент.", price_list=["Первые 7 дней: всё работает, кроме сохранения и экспорта","Подписка: сохранение проектов и экспорт без ограничений","Оплата и отмена через Stripe","Ключ проверяется на вашем компьютере, интернет для работы не нужен"],
  honest_h="Честно о возможностях", honest="Geometry построена на сетках (мешах), а не на точных поверхностях: экспорт STEP и DXF — гранёный. Для мелких деталей, 3D-печати и ЧПУ-эскизов этого достаточно; для сложных сборок и точных поверхностей — нет. Сборка проверена на macOS (Apple Silicon); сборки для других систем собираются на GitHub, но мы их не тестировали.",
  foot="© [ГОД] [ПРОДАВЕЦ]. Genix — независимый проект.",
 ),
 "en": dict(
  lang="en", prefix="/en", other="/", other_label="Русский", home="Home", terms_t="Terms of use", priv_t="Privacy", ref_t="Refunds",
  title="Genix — parametric CAD for Mac", desc="Native parametric CAD: sketches, features, history, STL, STEP, DXF export. 7 days free, then $0.99 a month.",
  h1="Parametric CAD that opens fast and feels good", lead="Genix is a native app for modelling parts: sketch, extrude, holes, fillets, patterns, multi-body work and export to STL, STEP, DXF. No cloud, no server account.",
  dl="Download for macOS", buy="Subscribe " + PRICE + " a month", shots=["Genix 0.00.46 model library", "A wine glass with a straw built in Genix"],
  feats_h="What it does", feats=[("Sketches","Lines, arcs, circles, ellipse, spline, text, dimensions and constraints, mirror and patterns."),("Features","Extrude with draft and thin wall, revolve, holes, fillet, chamfer, shell, thread."),("History","Every step stays editable and the model recomputes. Parameters and expressions."),("Bodies","Join, cut, intersect, exclusive, split, mirror, patterns, interference check."),("Measure","Distances, angles, face areas."),("Export","STL, 3MF, OBJ, STEP and DXF, open in your slicer.")],
  price_h="Price", price_lead="7 days free, then " + PRICE + " a month. Cancel any time.", price_list=["First 7 days: everything works except saving and export","Subscription: saving projects and export without limits","Payment and cancelling through Stripe","The key is checked on your computer; no internet needed to work"],
  honest_h="An honest note on capabilities", honest="Geometry is mesh-based rather than exact surfaces: STEP and DXF export is faceted. That suits small parts, 3D printing and CNC sketches; it does not suit complex assemblies or exact freeform surfaces. The app is verified on macOS (Apple Silicon); builds for other systems are produced on GitHub but were not tested by us.",
  foot="© [YEAR] [SELLER]. Genix is an independent project.",
 ),
}

LEGAL = {
 "terms": {"ru": ("Условия использования", """<p class="note">Шаблон, составленный без участия юриста. Перед запуском продаж его должен прочитать и поправить юрист вашей страны. Места в [КВАДРАТНЫХ СКОБКАХ] нужно заполнить.</p>
<h2>1. Кто мы</h2><p>Genix предоставляет [ПРОДАВЕЦ: имя или название, адрес, страна, регистрационный номер], далее «мы». Связь: [EMAIL ПОДДЕРЖКИ].</p>
<h2>2. Что вы получаете</h2><p>Право устанавливать и использовать Genix на своих устройствах. Пробный период — 7 дней с первого запуска; в нём нельзя сохранять проекты и экспортировать. Подписка стоит """+PRICE+""" в месяц (налоги могут добавляться в зависимости от страны) и открывает сохранение и экспорт, пока она действует.</p>
<h2>3. Оплата и отмена</h2><p>Оплату принимает Stripe. Подписка продлевается автоматически каждый месяц, пока вы её не отмените в окне «Подписка» → «Управление подпиской». Отмена действует со следующего периода; уже оплаченный период остаётся доступным.</p>
<h2>4. Возврат средств</h2><p>См. <a href="refunds.html">правила возврата</a>.</p>
<h2>5. Ваши модели</h2><p>Модели принадлежат вам. Мы не получаем ваши проекты: они хранятся на вашем компьютере. Вы отвечаете за резервные копии. После окончания подписки уже сохранённые файлы остаются у вас, но сохранять новые изменения без подписки нельзя.</p>
<h2>6. Важные ограничения</h2><p>Genix — инструмент проектирования. Результат (детали, чертежи, файлы для печати и станков) вы обязаны проверить сами перед изготовлением. Не используйте результат для изделий, где ошибка угрожает жизни или здоровью (медицина, авиация, несущие конструкции, защитное оборудование) без независимой инженерной проверки. Геометрия строится на сетках, экспорт STEP и DXF гранёный.</p>
<h2>7. Гарантии и ответственность</h2><p>Программа предоставляется «как есть», без гарантии безошибочной работы. В пределах, разрешённых законом, наша ответственность ограничена суммой, которую вы заплатили за последние 12 месяцев, и не включает упущенную выгоду и косвенные убытки. Это не ограничивает права, которые закон не позволяет ограничить (в том числе права потребителей).</p>
<h2>8. Запрещено</h2><p>Подделывать ключи, обходить пробный период или проверку лицензии, распространять свой ключ другим людям, использовать программу для незаконной деятельности.</p>
<h2>9. Сторонние компоненты</h2><p>Genix использует библиотеки с открытым кодом (в том числе egui, wgpu и другие) и адаптации визуальных эффектов React Bits; их лицензии сохранены в исходном коде проекта. Эти условия не отменяют лицензии этих компонентов.</p>
<h2>10. Изменения и право</h2><p>Мы можем менять условия, сообщив об этом на этой странице; изменения не касаются уже оплаченного периода. Применимое право и суд: [СТРАНА / ЮРИСДИКЦИЯ].</p>
<p class="small">Редакция от [ДАТА].</p>"""),
 "en": ("Terms of use", """<p class="note">A template written without a lawyer. A lawyer in your country must read and correct it before you sell anything. Fill in everything in [SQUARE BRACKETS].</p>
<h2>1. Who we are</h2><p>Genix is provided by [SELLER: name, address, country, registration number], "we". Contact: [SUPPORT EMAIL].</p>
<h2>2. What you get</h2><p>The right to install and use Genix on your own devices. The trial lasts 7 days from first launch; during it you cannot save projects or export. The subscription costs """+PRICE+""" a month (taxes may be added depending on your country) and enables saving and export while it is active.</p>
<h2>3. Payment and cancelling</h2><p>Payments are processed by Stripe. The subscription renews every month until you cancel it in the Subscription window → Manage subscription. Cancelling takes effect at the end of the paid period, which stays available.</p>
<h2>4. Refunds</h2><p>See the <a href="refunds.html">refund policy</a>.</p>
<h2>5. Your models</h2><p>Your models belong to you. We do not receive your projects; they stay on your computer. You are responsible for backups. After a subscription ends, files you already saved remain yours, but new changes cannot be saved without a subscription.</p>
<h2>6. Important limits</h2><p>Genix is a design tool. You must check the results (parts, drawings, print and machine files) yourself before making anything. Do not use the results for products where a mistake can endanger life or health (medical, aviation, load-bearing structures, protective equipment) without independent engineering review. Geometry is mesh-based and STEP/DXF export is faceted.</p>
<h2>7. Warranty and liability</h2><p>The software is provided "as is" with no warranty of error-free operation. To the extent the law allows, our liability is limited to the amount you paid in the last 12 months and excludes lost profit and indirect damages. This does not limit rights that the law does not allow to be limited, including consumer rights.</p>
<h2>8. Not allowed</h2><p>Forging keys, bypassing the trial or the licence check, sharing your key with others, or using the software for unlawful activity.</p>
<h2>9. Third-party components</h2><p>Genix uses open-source libraries (including egui, wgpu and others) and adaptations of React Bits visual effects; their licences are kept in the project source. These terms do not override those licences.</p>
<h2>10. Changes and law</h2><p>We may change these terms by posting on this page; changes do not affect an already paid period. Governing law and courts: [COUNTRY / JURISDICTION].</p>
<p class="small">Version of [DATE].</p>""")},
 "privacy": {"ru": ("Политика конфиденциальности", """<p class="note">Шаблон без участия юриста: проверьте его (особенно для ЕС и Великобритании: GDPR) и заполните [КВАДРАТНЫЕ СКОБКИ].</p>
<h2>Кратко</h2><p>Программа работает на вашем компьютере. Мы не собираем статистику использования, не читаем и не загружаем ваши модели.</p>
<h2>Что хранится у вас</h2><p>Настройки, локальный профиль, проекты и отметка начала пробного периода (файл <code>.genix-trial</code> в домашней папке). Всё это остаётся на вашем компьютере.</p>
<h2>Что получаем мы и наши подрядчики</h2><ul><li><b>Оплата.</b> Stripe получает e-mail, платёжные данные и страну; данные карты мы не видим и не храним. Политика Stripe: stripe.com/privacy.</li><li><b>Лицензионный сервер.</b> При покупке, обновлении ключа и открытии управления подпиской сервер получает ваш ключ (в нём e-mail и идентификатор клиента Stripe) и запрашивает у Stripe статус подписки. Своей базы данных у сервера нет. Хостинг сервера: [ХОСТИНГ, например Cloudflare]; он может вести технические журналы (IP-адрес, время).</li><li><b>Проверка обновлений.</b> Только когда вы сами нажимаете «Проверить обновления», программа обращается к GitHub.</li><li><b>Отчёт о проблеме.</b> Отправляется только если вы сами решили его отправить.</li></ul>
<h2>Основания и сроки</h2><p>Данные платежей нужны для исполнения договора и учёта; храним столько, сколько требует закон о бухгалтерии: [СРОК].</p>
<h2>Ваши права</h2><p>Вы можете запросить доступ, исправление и удаление данных, написав на [EMAIL ПОДДЕРЖКИ]. Жалобы можно подать в надзорный орган вашей страны.</p>
<h2>Дети</h2><p>Программа не предназначена для детей младше [ВОЗРАСТ].</p>
<p class="small">Редакция от [ДАТА]. Ответственный за данные: [ПРОДАВЕЦ].</p>"""),
 "en": ("Privacy policy", """<p class="note">A template written without a lawyer: have it checked (especially for the EU and UK: GDPR) and fill in [SQUARE BRACKETS].</p>
<h2>In short</h2><p>The app runs on your computer. We collect no usage statistics and we do not read or upload your models.</p>
<h2>What stays with you</h2><p>Settings, the local profile, projects and the trial-start marker (the <code>.genix-trial</code> file in your home folder). All of it stays on your computer.</p>
<h2>What we and our providers receive</h2><ul><li><b>Payment.</b> Stripe receives your e-mail, payment details and country; we never see or store card data. Stripe's policy: stripe.com/privacy.</li><li><b>Licence server.</b> When you buy, renew a key or open subscription management, the server receives your key (it holds your e-mail and Stripe customer id) and asks Stripe for the subscription status. The server has no database of its own. Server hosting: [HOST, e.g. Cloudflare]; it may keep technical logs (IP address, time).</li><li><b>Update check.</b> Only when you press "Check for updates" does the app contact GitHub.</li><li><b>Problem report.</b> Sent only if you choose to send it.</li></ul>
<h2>Basis and retention</h2><p>Payment data is needed to perform the contract and for accounting; we keep it as long as accounting law requires: [PERIOD].</p>
<h2>Your rights</h2><p>You can ask for access, correction and deletion by writing to [SUPPORT EMAIL]. You may complain to the supervisory authority in your country.</p>
<h2>Children</h2><p>The app is not intended for children under [AGE].</p>
<p class="small">Version of [DATE]. Data controller: [SELLER].</p>""")},
 "refunds": {"ru": ("Возврат средств", """<p class="note">Выберите политику сами и согласуйте с юристом. Ниже — разумный черновик.</p>
<h2>Пробный период</h2><p>Первые 7 дней бесплатны именно для того, чтобы вы проверили, подходит ли программа, до оплаты.</p>
<h2>Возврат</h2><p>Если вы оплатили подписку и передумали, напишите на [EMAIL ПОДДЕРЖКИ] в течение [14] дней с оплаты — вернём платёж за этот месяц. Если программа не работает у вас по нашей вине и мы не смогли это исправить, вернём оплату за период, в котором это было.</p>
<h2>Отмена</h2><p>Отменить подписку можно в любой момент: окно «Подписка» → «Управление подпиской». Следующий платёж не спишется.</p>
<h2>Потребители в ЕС и других странах</h2><p>Если закон вашей страны даёт вам право отказаться от договора или на возврат, оно сохраняется.</p>"""),
 "en": ("Refunds", """<p class="note">Pick your policy yourself and check it with a lawyer. This is a reasonable draft.</p>
<h2>Trial</h2><p>The first 7 days are free precisely so you can check the app fits before paying.</p>
<h2>Refunds</h2><p>If you paid and changed your mind, write to [SUPPORT EMAIL] within [14] days of the payment and we will refund that month. If the app does not work for you because of our fault and we could not fix it, we will refund the period concerned.</p>
<h2>Cancelling</h2><p>You can cancel at any time: Subscription window → Manage subscription. The next payment will not be charged.</p>
<h2>Consumers in the EU and elsewhere</h2><p>Any right to withdraw or to a refund that the law of your country gives you stays in place.</p>""")},
}

def shell(t, title, body, here):
    p = t["prefix"]
    other = t["other"] + here
    return f"""<!doctype html><html lang="{t['lang']}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title><link rel="icon" href="/favicon.png"><meta name="description" content="{t['desc']}"><style>{CSS}</style></head><body>
<header><a class="logo" href="{p}/"><img src="/img/logo.png" alt="" width="28" height="28" style="vertical-align:-7px;border-radius:7px;margin-right:8px">Genix</a><nav><a href="{p}/terms.html">{t['terms_t']}</a><a href="{p}/privacy.html">{t['priv_t']}</a><a href="{other}">{t['other_label']}</a></nav></header>
{body}
<footer><a href="{p}/terms.html">{t['terms_t']}</a><a href="{p}/privacy.html">{t['priv_t']}</a><a href="{p}/refunds.html">{t['ref_t']}</a><p>{t['foot']}</p></footer></body></html>"""

def index(t):
    imgs = [("library", "jpg"), ("glass", "png")]
    shots = "".join(f'<img class="shot" src="/img/{n}.{e}" alt="{a}" loading="lazy">' for (n, e), a in zip(imgs, t["shots"]))
    feats = "".join(f'<div class="card"><h3>{h}</h3><p>{p}</p></div>' for h, p in t["feats"])
    price = "".join(f"<li>{x}</li>" for x in t["price_list"])
    body = f"""<main><section class="hero"><h1>{t['h1']}</h1><p class="lead">{t['lead']}</p>
<a class="btn" href="{REPO}">{t['dl']}</a><a class="btn alt" href="#price">{t['buy']}</a></section>{shots}
<h2>{t['feats_h']}</h2><div class="grid">{feats}</div>
<h2 id="price">{t['price_h']}</h2><p class="price">{PRICE}<span class="small"> / {'месяц' if t['lang']=='ru' else 'month'}</span></p><p>{t['price_lead']}</p><ul class="tick">{price}</ul>
<h2>{t['honest_h']}</h2><p>{t['honest']}</p></main>"""
    return shell(t, t["title"], body, "")

def main():
    for lang, t in T.items():
        out = ROOT if lang == "ru" else ROOT / "en"
        out.mkdir(parents=True, exist_ok=True)
        (out / "index.html").write_text(index(t), encoding="utf-8")
        for page, texts in LEGAL.items():
            title, inner = texts[lang]
            (out / f"{page}.html").write_text(shell(t, f"{title} — Genix", f'<main class="legal"><h1>{title}</h1>{inner}</main>', f"{page}.html"), encoding="utf-8")
    print("site built:", sorted(str(p.relative_to(ROOT)) for p in ROOT.rglob("*.html")))

if __name__ == "__main__":
    main()
