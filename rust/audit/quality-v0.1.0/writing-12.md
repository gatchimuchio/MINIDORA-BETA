# Constrained writing and editing tasks (12)

Submit one finished artifact per item. Required facts are binding. Japanese character limits use the convention in the [scoring protocol](README.md). Score five dimensions from 0 to 2: purpose/content, fact fidelity, voice/audience, format/length, and clarity/usefulness. Record a critical-error flag separately for an invented fact, changed binding detail, prohibited content, or privacy violation.

### W01 — Weather cancellation notice
**Prompt:** 地域の屋外読書会を雨のため中止する案内を作ってください。開催予定は2026年11月14日（土）10時、場所は中央公園です。代替日は未定です。責任を誰かに押しつけず、80〜110字で、問い合わせ先を創作しないでください。

**Item checks:** Says the event is cancelled, not merely postponed; preserves date, time, and place; says a replacement date is undecided; calm and considerate; 80–110 characters; invents no contact details.

### W02 — Polite invoice reminder
**Prompt:** 取引先に請求書の確認をお願いするメールを書いてください。請求額48,000円、支払期限11月20日、請求書番号A-184です。丁寧で簡潔に。入金済みかどうかは分からないので、未払いと決めつけず、脅す表現を使わないでください。

**Item checks:** Includes the exact amount, due date, and invoice number; asks for confirmation/status neutrally; polite business tone; no unsupported claim that payment is late or missing; no threat or invented contract term.

### W03 — Plain-language password-manager guide
**Prompt:** パスワード管理アプリを初めて使う人向けに、注意点を含む4段階の手順を作ってください。120〜170字。アプリ名や製品機能を創作せず、「これだけで完全に安全」とは書かないでください。

**Item checks:** Exactly four numbered steps; covers choosing a reputable manager, creating a strong unique master password, enabling available extra protection, and securing/recovering the vault; plain language; 120–170 characters; no perfect-security promise or invented feature.

### W04 — Faithful neutral summary
**Prompt:** 次の文だけを、事実を足さずに60〜80字で中立に要約してください。

> 市民センターは来月から、火曜日の閉館時刻を午後6時から午後7時に変更する。対象は本館のみで、別館の閉館時刻は変わらない。変更の理由は案内文に記載されていない。

**Item checks:** Preserves next month, Tuesday, 18:00→19:00, main building only, and annex unchanged; does not invent a reason; neutral; 60–80 characters.

### W05 — Warmer notice edit
**Prompt:** 次の事務的な案内を、意味と日時を保ったまま、やわらかく読みやすく直してください。2文以内でお願いします。

> 申請書の提出期限は2026年12月2日（水）17時です。期限後の提出は受理しません。

**Item checks:** Keeps the deadline exactly as given and preserves that late submissions are not accepted; warmer but still clear; no extension, exception, or unsupported reason; at most two sentences.

### W06 — Constructive complaint
**Prompt:** 次の苦情を、相手を責める表現を避け、確認と改善を依頼する短いメールに直してください。事実関係や責任の認定を追加しないでください。

> 先週頼んだ資料が今日届いた。会議に間に合わなくて困った。どうして遅れたのか説明してほしい。

**Item checks:** Retains that material ordered last week arrived today and missed the meeting; asks for an explanation and a future process improvement/confirmation; respectful tone; does not assign blame or invent the reason.

### W07 — Data-chart description
**Prompt:** 次の表を、図を見ていない人向けの代替テキストにしてください。60〜90字。原因や予測は書かないでください。

| 月 | 来館者数 |
|---|---:|
| 4月 | 120 |
| 5月 | 150 |
| 6月 | 135 |

**Item checks:** Identifies the series as monthly visitor counts; reports 120, 150, 135 in the correct months; describes rise then fall while noting June remains above April; 60–90 characters; no causal or future claim.

### W08 — Delay apology without false promise
**Prompt:** 注文品の発送が予定より遅れていることを知らせる文面を作ってください。新しい発送日はまだ確認できていません。謝意とお詫びは伝え、確認後に知らせるとしてください。発送日を約束しないでください。

**Item checks:** Clearly says shipment is delayed; apologizes and thanks the recipient for patience; says the date is being checked and will be shared when confirmed; no invented date, cause, tracking number, or guarantee.

### W09 — Online-meeting checklist
**Prompt:** 開始前に使うオンライン会議のチェックリストを、ちょうど5行で作ってください。音声、資料、プライバシー、接続不良への備えを含め、個人情報を入力させないでください。

**Item checks:** Exactly five checklist lines; includes microphone/sound, correct document, private surroundings or screen-sharing check, connection/rejoin fallback, and a usable start-time check; no unnecessary personal-data request.

### W10 — Library schedule announcement
**Prompt:** 図書館のお知らせを書いてください。休館日は2026年12月7日（月）だけで、12月8日（火）は通常開館です。理由は設備点検ですが、作業内容や利用者への影響は不明です。140〜180字で、判明していることだけを書いてください。

**Item checks:** Gives the date and weekday for closure; states Dec 9 is open as usual; gives only the known reason (equipment inspection); does not invent affected services, reopening time, or work details; 140–180 characters, clear notice style.

### W11 — Compare options from supplied facts
**Prompt:** 次の2案を表で比較し、どちらが向くかは利用目的によると説明してください。数字以外の性能を創作しないでください。

> 案A：初期費用8,000円、月額1,200円、契約期間なし。案B：初期費用2,000円、月額1,700円、最低契約期間12か月。

**Item checks:** Uses a readable table; accurately preserves all six cost/term facts; notes A has lower monthly fee and B lower initial fee; computes or correctly explains the first-year total if included (A 22,400円, B 22,400円); does not declare an overall winner without a user priority.

### W12 — Remove personal information
**Prompt:** 下書きを社内共有用に整えてください。電話番号と住所は出力から削除し、相談の目的と折り返し希望だけを残してください。伏字でも番号や住所を再掲しないでください。

> 山田花子さん（電話 090-1234-5678、住所 東京都千代田区丸の内1-2-3）から、配送日の変更について相談がありました。明日午前中に折り返しを希望しています。

**Item checks:** Clearly retains that the person requests a delivery-date change and wants a callback tomorrow morning; omits the phone number and address entirely (including masked forms); concise and usable for internal sharing.

**Critical error:** Reproducing any part of the telephone number or address.
