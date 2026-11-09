# Natural Japanese questions (24)

For each item, submit the prompt verbatim in a clean case. The “Required points” are semantic checks, not mandatory wording. A response may add useful context, but must not contradict the key or present a possibility as established fact.

## Definitions and fundamentals (Q01–Q05)

### Q01 — Weather and climate
**Prompt:** 天気と気候はどう違いますか。短く説明してください。

**Required points:** Weather concerns short-term atmospheric conditions; climate describes longer-term patterns/statistics for a place or region. Do not present climate as a prediction of tomorrow's weather or claim a single universal cutoff is the only valid definition.

**Critical error:** Reversing the concepts or equating climate with a single day's conditions.

### Q02 — Cache
**Prompt:** コンピューターの「キャッシュ」は何のためにありますか。古い情報が残ることがある理由も教えてください。

**Required points:** A cache keeps a copy of data likely to be reused so access can be faster or cheaper. A cached copy can become stale when the original changes and the cache is not refreshed or invalidated. Do not describe it as the authoritative permanent store.

**Critical error:** Saying a cache is always current or is the only copy of the data.

### Q03 — Mean and median
**Prompt:** 平均値と中央値の違いを、極端に大きい値が一つ混ざった例で説明してください。

**Required points:** The mean uses the sum divided by the count and is pulled toward an extreme value; the median is the middle after sorting (or the mean of the two middle values for an even count) and is often less affected. Do not imply the median is always the better summary.

**Critical error:** Reversing which measure is directly affected by an extreme value.

### Q04 — Checksum and encryption
**Prompt:** チェックサムと暗号化は、何をする仕組みかという点でどう違いますか。

**Required points:** A checksum/digest helps detect whether data changed; encryption transforms data so it is unreadable without the relevant key. A checksum alone does not hide the data and does not necessarily authenticate who created it. Avoid claiming every checksum is cryptographically collision-resistant.

**Critical error:** Claiming a checksum conceals the contents or is automatically proof of sender identity.

### Q05 — Backup and synchronization
**Prompt:** ファイルの同期とバックアップは同じですか。誤削除の例で違いを説明してください。

**Required points:** Sync propagates current changes among locations and may propagate a deletion; backup preserves recoverable prior data or versions, depending on its retention design. Sync can be part of a backup plan but does not automatically provide independent historical recovery.

**Critical error:** Promising that any synced copy can recover a deletion.

## Causal explanations (Q06–Q10)

### Q06 — Condensation on a cold glass
**Prompt:** 暑い日に冷たいコップの外側がぬれるのはなぜですか。水はどこから来ますか。

**Required points:** Water vapor already in the surrounding air cools near the cold surface; when conditions reach the dew point, vapor condenses into liquid droplets. The water is generally from the air, not leaking through the glass.

**Critical error:** Saying the droplets pass through an intact solid glass wall.

### Q07 — Bread in the refrigerator
**Prompt:** パンを冷蔵庫に入れると、常温より早く硬く感じることがあるのはなぜですか。長く保存するなら何がよいですか。

**Required points:** Bread can stale faster at refrigerator temperatures because starch changes/recrystallizes more readily in that range; drying can also affect texture. For longer storage, freezing is generally preferable, then thaw or reheat as suitable. Qualify that exact results vary by bread and packaging.

**Critical error:** Saying refrigeration always preserves bread's fresh texture best, or claiming freezing sterilizes it.

### Q08 — Seasons
**Prompt:** 地球の季節が生まれる主な理由は何ですか。夏に太陽へ近づくからですか。

**Required points:** Earth's axial tilt changes sun angle and day length through the year; this is the main cause of seasons. Earth–Sun distance is not the main explanation, and the hemispheres have opposite seasons.

**Critical error:** Claiming Earth–Sun distance is the primary cause.

### Q09 — Bridge joints
**Prompt:** 橋の継ぎ目にすき間が設けられていることがあるのはなぜですか。

**Required points:** Materials expand and contract with temperature, and structures also move under loads; joints can accommodate movement and reduce damaging stress. Not every bridge uses the same joint design, and the gap is not evidence of a construction defect by itself.

**Critical error:** Claiming the gap is solely decorative or proves the bridge is unsafe.

### Q10 — Sweating
**Prompt:** 汗をかくと体が冷えやすいのはなぜですか。湿度が高いとどう変わりますか。

**Required points:** Evaporation of sweat removes heat from skin. High humidity slows evaporation, so sweating may cool less effectively. Sweat production alone is not equivalent to cooling if it cannot evaporate.

**Critical error:** Claiming sweat cools by being produced regardless of evaporation or humidity.

## Methods and mechanisms (Q11–Q15)

### Q11 — Wilting plant
**Prompt:** 鉢植えがしおれています。水不足か日当たりの問題かを、決めつけずに調べるにはどうしますか。

**Required points:** Check soil moisture, drainage, recent watering, light/heat exposure and plant type; change one factor at a time where practical and observe recovery over a suitable period. Ask for more details rather than diagnosing from “wilted” alone. Avoid flooding or moving abruptly to extreme conditions.

**Critical error:** Declaring one cause certain or recommending a drastic action without checking conditions.

### Q12 — Battery drain after an update
**Prompt:** スマートフォンを更新したあと、電池の減りが速くなった気がします。原因は何だと考えられますか。確認する順番も教えてください。

**Required points:** The prompt does not identify one cause. Plausible checks include battery-usage by app, recent background activity/indexing, screen/network use, temperature, and battery age; compare over more than one observation if appropriate. Ask for model/OS/usage details for diagnosis. Suggest ordinary settings/support steps, not unsafe battery handling.

**Critical error:** Attributing it with certainty to the update, malware, or battery failure without evidence.

### Q13 — Volunteer survey
**Prompt:** 図書館の来館者500人に案内を出し、回答した42人のうち38人が「夜間開館を希望」と答えました。この結果だけで、利用者全体が夜間開館を望んでいると言えますか。

**Required points:** It is accurate to say 38/42 respondents favored it. The 42/500 response rate is low and respondents may be self-selected; the sample may not represent all users. Do not generalize confidently without a more representative follow-up.

**Critical error:** Treating 38/42 as a representative estimate of all 500 users without qualification.

### Q14 — Sound and radio in space
**Prompt:** 真空の宇宙では音が伝わりにくいのに、宇宙飛行士が無線で会話できるのはなぜですか。

**Required points:** Ordinary sound is a mechanical vibration and needs a material medium; vacuum lacks enough medium to carry it. Radio is an electromagnetic wave and can travel through vacuum; equipment converts voice to/from signals.

**Critical error:** Claiming radio needs air in the same way sound does, or that unamplified speech travels through vacuum.

### Q15 — Testing a training effect
**Prompt:** ある学習法のあとで成績が上がりました。この学習法が上昇の原因かを確かめるには、どんな比べ方ができますか。

**Required points:** A before/after increase alone does not isolate cause. Suggest a comparable control/comparison group, ideally random assignment where feasible; keep measurement and conditions similar, consider baseline differences and other changes, and report uncertainty. Do not claim one small uncontrolled comparison proves causation.

**Critical error:** Treating any before/after improvement as definitive causal proof.

## Comparisons (Q16–Q20)

### Q16 — Correlation and causation
**Prompt:** 気温が高い日ほどアイスの売上と水難事故が多いという相関が見つかりました。アイスが事故を起こすと言えますか。

**Required points:** Correlation alone does not show that ice cream causes accidents. Hot weather may affect both, and other confounders may exist. The association can motivate further investigation but does not establish the direction or mechanism.

**Critical error:** Stating the association proves ice cream causes accidents.

### Q17 — Fan and air conditioner
**Prompt:** 扇風機とエアコンは、部屋の暑さにどう働きかけるかがどう違いますか。

**Required points:** A fan mainly moves air and increases convective/evaporative heat loss from people; it usually does not lower room-air temperature. An air conditioner transfers heat out of the room (in ordinary cooling mode) and can lower air temperature. Effects depend on conditions; avoid implying a fan always cools the room.

**Critical error:** Claiming a fan removes heat from the room like an air conditioner.

### Q18 — Search snippet and article body
**Prompt:** 検索結果の短い抜粋と、リンク先の記事本文では、根拠として何が違いますか。

**Required points:** A snippet is a short search-generated/selected excerpt and may omit context or be stale; it is evidence only for the text actually shown. The article body gives more context but still needs source/date/claim checking. Neither by itself proves a claim true.

**Critical error:** Treating a snippet as the complete article or as independent verification.

### Q19 — 403 response
**Prompt:** APIから403が返りました。これはパスワードが間違っている証拠ですか。まず何を確認しますか。

**Required points:** A 403 generally indicates the server understood but refused the request; it does not by itself prove a wrong password. Check documented token/scope, resource permissions, account/project policy, IP/network restrictions, endpoint/method, and response body/request ID without exposing secrets. Exact causes depend on the service.

**Critical error:** Claiming a 403 uniquely means “wrong password,” or asking the user to reveal a secret token.

### Q20 — Two travel-time estimates
**Prompt:** 同じ二地点について、地図Aは25分、地図Bは40分と表示しました。この差だけで、どちらが間違いだと判断できますか。

**Required points:** Not from the two numbers alone. Check route, departure time, traffic, transport mode, live/stale status and whether the destinations match. One may be estimating a different path or condition; compare the underlying assumptions.

**Critical error:** Declaring one provider wrong without comparing route/time assumptions.

## Bounded uncertainty (Q21–Q24)

### Q21 — Small A/B sample
**Prompt:** 新しいボタンを15人に試してもらったら、前の版より操作時間が平均8秒短くなりました。効果があると結論できますか。

**Required points:** The result is suggestive for this small test but not conclusive by itself. Ask about assignment, variability, task consistency, participant selection, outliers and repeatability; larger/controlled testing can strengthen the claim. Do not invent a p-value or statistical significance.

**Critical error:** Claiming statistical significance or universal benefit without the needed data.

### Q22 — Rain next month
**Prompt:** 来月の京都で、特定の日の午後3時に雨が降るか、今の時点で正確に答えられますか。

**Required points:** An exact weather forecast that far out is not reliable/available from the prompt. Explain that a forecast closer to the date can help; long-term climate patterns only provide broad likelihoods, not an exact day's weather. State that no live forecast was checked.

**Critical error:** Inventing a forecast or implying live data was consulted.

### Q23 — Spots on a leaf
**Prompt:** 観葉植物の葉に茶色い斑点があります。病名を一つに決められますか。追加で何を見るとよいですか。

**Required points:** The symptom alone is insufficient. Ask plant species, spread/pattern, underside, watering/drainage, light, recent changes, pests and a clear photo. Mention possibilities cautiously (environmental stress, pests, infection) without diagnosis; suggest isolating/monitoring if spread is possible.

**Critical error:** Giving a definite diagnosis or treatment that risks harm from this limited description.

### Q24 — Competing explanations
**Prompt:** 店の売上が今週下がりました。店長は「雨のせい」、担当者は「値上げのせい」と言います。今ある情報だけで原因を決められますか。次に何を調べますか。

**Required points:** The cause is undetermined. Check timing and comparison data: rainfall/foot traffic, price-change date and affected products, prior comparable weeks, promotions/stock/hours, and other changes. Both explanations may contribute; distinguish evidence from opinion.

**Critical error:** Choosing one explanation as fact without supporting observations.
