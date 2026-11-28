//! Deterministic fictional narrative planning and realization on the Core generation path.
use minidora_domain::{
    FictionAudience, FictionGenre, FictionRegister, FictionRequest, FictionTone, Result, failure,
    identity,
};
use serde::{Deserialize, Serialize};

use crate::language::{
    MAX_EXACT_FIELD_GENERATION_BYTES, MAX_EXACT_FIELD_GENERATION_TOKENS, MeaningCompositionFormat,
    MeaningCompositionPurpose, MeaningCompositionStyle, MeaningGenerationReport,
    MeaningGenerationRequest, MeaningGrammarFragment, MeaningGrammarUnit, MeaningRole,
};

pub(crate) const ARTIFACT_KIND: &str = "minidora.core-computation.fiction.v1";
pub(crate) const LABEL: &str = "【創作】\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum StoryArc {
    Retrace,
    Reunite,
    Signal,
    Remember,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Event {
    Arrival,
    Scene,
    Problem,
    Discovery,
    Action,
    TurningPoint,
    Resolution,
    Reflection,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FictionMeaningPlan {
    source_bundle: String,
    draft: FictionRequest,
    arc: StoryArc,
    events: Vec<Event>,
    protagonist: String,
    setting: String,
    elements: Vec<String>,
}

impl FictionMeaningPlan {
    pub(crate) fn from_draft(draft: FictionRequest, source_bundle: String) -> Result<Self> {
        let count = draft.sentence_count;
        let protagonist = draft.protagonist.as_ref().map(|span| span.text.as_str());
        let setting = draft.setting.as_ref().map(|span| span.text.as_str());
        let elements: Vec<_> = draft
            .required_elements
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        if !(1..=8).contains(&count)
            || elements.len() > 3
            || protagonist
                .into_iter()
                .chain(setting)
                .chain(elements.iter().copied())
                .any(|field| {
                    field.trim().is_empty()
                        || field.len() > 256
                        || field.contains(['。', '！', '？', '!', '?', '\r', '\n'])
                        || field.chars().any(char::is_control)
                })
        {
            return Err(failure(
                "FICTION_BRIEF",
                "物語の設定または文数が対応範囲を超えています。",
            ));
        }
        // Equivalent briefs choose the same arc even when their source spans move.
        let digest = identity(&(protagonist, setting, &elements, draft.tone, draft.genre))?;
        let index = digest.as_bytes()[0] as usize;
        let arc = match draft.tone {
            FictionTone::Warm => [StoryArc::Reunite, StoryArc::Signal][index % 2],
            FictionTone::Mysterious => [StoryArc::Retrace, StoryArc::Remember][index % 2],
            FictionTone::Playful => [StoryArc::Signal, StoryArc::Retrace][index % 2],
            FictionTone::Neutral => [
                StoryArc::Retrace,
                StoryArc::Reunite,
                StoryArc::Signal,
                StoryArc::Remember,
            ][index % 4],
        };
        let protagonist = protagonist
            .unwrap_or(["ハル", "ミツ", "ユキ", "レン"][index % 4])
            .to_owned();
        let setting = setting
            .unwrap_or(["夕暮れの広場", "朝の川辺", "雨上がりの町", "静かな通り"][index % 4])
            .to_owned();
        let elements = if elements.is_empty() {
            vec![["黄色いリボン", "小さな旗", "赤い風車", "青い包み"][index % 4].to_owned()]
        } else {
            elements.into_iter().map(str::to_owned).collect()
        };
        let events = [
            (Event::Arrival, 1),
            (Event::Scene, 7),
            (Event::Problem, 1),
            (Event::Discovery, 5),
            (Event::Action, 1),
            (Event::TurningPoint, 6),
            (Event::Resolution, 1),
            (Event::Reflection, 8),
        ]
        .into_iter()
        .filter_map(|(event, required)| (count >= required).then_some(event))
        .collect();
        Ok(Self {
            source_bundle,
            draft,
            arc,
            events,
            protagonist,
            setting,
            elements,
        })
    }

    pub(crate) fn dependencies(&self) -> Vec<String> {
        vec![self.source_bundle.clone()]
    }

    pub(crate) fn grammar(&self, budget: usize) -> Result<MeaningGenerationRequest> {
        let young = self.draft.audience == FictionAudience::YoungChildren;
        let elementary = self.draft.audience == FictionAudience::Elementary;
        let polite = self.draft.register == FictionRegister::Polite;
        let choose = |slot: &str, alternatives: &[&str]| MeaningGrammarFragment::Choice {
            slot: slot.into(),
            alternatives: alternatives.iter().map(|text| (*text).to_owned()).collect(),
        };
        let mut fragments = vec![exact(LABEL)];
        let merged = self.events.len() - usize::from(self.draft.sentence_count);
        for (index, event) in self.events.iter().enumerate() {
            match (event, self.draft.tone, self.draft.audience) {
                (Event::Problem, FictionTone::Mysterious, _) => {
                    fragments.push(literal("妙なことに、"));
                }
                (Event::Action, FictionTone::Playful, _) => {
                    fragments.push(literal("遊びの続きのように、"));
                }
                (Event::Resolution, _, FictionAudience::Adult) if index >= merged => {
                    fragments.push(literal("やがて、"));
                }
                _ => {}
            }
            if *event == Event::Arrival {
                if young || self.draft.genre == FictionGenre::FairyTale {
                    fragments.push(literal("ある日、"));
                } else if elementary {
                    fragments.push(literal("ある日のこと、"));
                }
                fragments.extend([
                    exact(&self.protagonist),
                    literal("は"),
                    exact(&self.setting),
                ]);
            }
            if *event == Event::Action {
                for (index, element) in self.elements.iter().enumerate() {
                    if index != 0 {
                        fragments.push(literal("と"));
                    }
                    fragments.push(exact(element));
                }
            }
            if *event == Event::Scene {
                fragments.push(match self.draft.tone {
                    FictionTone::Mysterious => choose(
                        "scene",
                        &["空気が張りつめたようで、", "物音が消えたようで、"],
                    ),
                    FictionTone::Playful => {
                        choose("scene", &["なんだか楽しくて、", "胸が弾んで、"])
                    }
                    FictionTone::Warm => {
                        choose("scene", &["やわらかな光の中で、", "おだやかな空気の中で、"])
                    }
                    FictionTone::Neutral => {
                        choose("scene", &["そのあたりには、", "すぐそばでは、"])
                    }
                });
            }
            let (plain, polite_text, linked) = self.event_surface(*event, young || elementary);
            fragments.push(literal(if index < merged {
                linked
            } else if polite {
                polite_text
            } else {
                plain
            }));
        }
        Ok(MeaningGenerationRequest {
            purpose: MeaningCompositionPurpose::ShortComposition,
            format: MeaningCompositionFormat::Paragraph,
            style: MeaningCompositionStyle::Neutral,
            units: vec![MeaningGrammarUnit {
                id: "fiction-narrative".into(),
                role: MeaningRole::Body,
                evidence_ids: self.dependencies(),
                dependencies: Vec::new(),
                semantic_sha256: identity(&(ARTIFACT_KIND, self))?,
                fragments,
            }],
            max_output_bytes: budget.min(MAX_EXACT_FIELD_GENERATION_BYTES),
            max_tokens: MAX_EXACT_FIELD_GENERATION_TOKENS,
        })
    }

    fn event_surface(
        &self,
        event: Event,
        for_children: bool,
    ) -> (&'static str, &'static str, &'static str) {
        use Event::*;
        match event {
            Arrival => ("に来た。", "に来ました。", "に来て、"),
            Scene => (
                "しばらく耳をすませた。",
                "しばらく耳をすませました。",
                "しばらく耳をすますと、",
            ),
            Reflection if self.draft.audience == FictionAudience::Adult => match self.arc {
                StoryArc::Remember => (
                    "見慣れた場所のはずなのに、その日だけのものに思えた。",
                    "見慣れた場所のはずなのに、その日だけのものに思えました。",
                    "",
                ),
                _ => (
                    "振り返って初めて、さっきまで肩に力が入っていたことに気づいた。",
                    "振り返って初めて、さっきまで肩に力が入っていたことに気づきました。",
                    "",
                ),
            },
            Reflection => (
                "ほっとして、ひと息ついた。",
                "ほっとして、ひと息つきました。",
                "",
            ),
            _ => self.arc_surface(event, for_children),
        }
    }

    fn arc_surface(
        &self,
        event: Event,
        for_children: bool,
    ) -> (&'static str, &'static str, &'static str) {
        use Event::*;
        use StoryArc::*;
        match (self.arc, event) {
            (Retrace, Problem) if for_children => (
                "帰りみちがわからなくなった。",
                "帰りみちがわからなくなりました。",
                "帰りみちがわからなくなったけれど、",
            ),
            (Retrace, Problem) => (
                "来た道を見失った。",
                "来た道を見失いました。",
                "来た道を見失ったものの、",
            ),
            (Retrace, Discovery) => (
                "来る途中に見たものを思い出した。",
                "来る途中に見たものを思い出しました。",
                "",
            ),
            (Retrace, Action) => (
                "を目印に、歩いた順番を考えた。",
                "を目印に、歩いた順番を考えました。",
                "を目印に歩いた順番を考え、",
            ),
            (Retrace, TurningPoint) => (
                "同じような景色でも、目印の並びは違っていた。",
                "同じような景色でも、目印の並びは違っていました。",
                "",
            ),
            (Retrace, Resolution) => (
                "順番を逆にたどると、来た道に戻れた。",
                "順番を逆にたどると、来た道に戻れました。",
                "",
            ),
            (Reunite, Problem) => (
                "置き去りにされたものを見つけた。",
                "置き去りにされたものを見つけました。",
                "置き去りにされたものを見つけて、",
            ),
            (Reunite, Discovery) => (
                "近くで、何かをさがす声がした。",
                "近くで、何かをさがす声がしました。",
                "",
            ),
            (Reunite, Action) => (
                "のそばで足を止め、落とし物をした人に声をかけた。",
                "のそばで足を止め、落とし物をした人に声をかけました。",
                "のそばで足を止め、持ち主らしい人に声をかけ、",
            ),
            (Reunite, TurningPoint) => (
                "あたりを見回していた人が、その声に振り返った。",
                "あたりを見回していた人が、その声に振り返りました。",
                "",
            ),
            (Reunite, Resolution) => (
                "落とし物の場所を知らせると、持ち主の顔がほころんだ。",
                "落とし物の場所を知らせると、持ち主の顔がほころびました。",
                "",
            ),
            (Signal, Problem) => (
                "遠くに人がいたが、呼んでも声がとどかなかった。",
                "遠くに人がいましたが、呼んでも声がとどきませんでした。",
                "遠くに人がいたが声がとどかず、",
            ),
            (Signal, Discovery) => (
                "声のかわりになる目印をさがした。",
                "声のかわりになる目印をさがしました。",
                "",
            ),
            (Signal, Action) => (
                "のそばへ移り、その場所が分かるように手を振った。",
                "のそばへ移り、その場所が分かるように手を振りました。",
                "のそばへ移って大きく手を振ると、",
            ),
            (Signal, TurningPoint) => (
                "相手は一度通りすぎてから、ふと足を止めた。",
                "相手は一度通りすぎてから、ふと足を止めました。",
                "",
            ),
            (Signal, Resolution) => (
                "相手はこちらに気づき、同じように手を振ってきた。",
                "相手はこちらに気づき、同じように手を振ってきました。",
                "",
            ),
            (Remember, Problem) => (
                "目の前の景色を覚えて帰りたかったが、何から覚えればよいか迷った。",
                "目の前の景色を覚えて帰りたかったのですが、何から覚えればよいか迷いました。",
                "目の前の景色を覚えて帰りたいと思い、",
            ),
            (Remember, Discovery) => (
                "全部を思い出そうとすると、かえって細かいところがぼやけた。",
                "全部を思い出そうとすると、かえって細かいところがぼやけました。",
                "",
            ),
            (Remember, Action) => (
                "を手がかりに、忘れたくない景色を心に描いた。",
                "を手がかりに、忘れたくない景色を心に描きました。",
                "を手がかりに、忘れたくない景色を心に描くと、",
            ),
            (Remember, TurningPoint) => (
                "覚えた部分を思い浮かべると、その向こうの景色までよみがえった。",
                "覚えた部分を思い浮かべると、その向こうの景色までよみがえりました。",
                "",
            ),
            (Remember, Resolution) => (
                "帰り道でも、目を閉じればその場所がありありと思い浮かんだ。",
                "帰り道でも、目を閉じればその場所がありありと思い浮かびました。",
                "",
            ),
            (_, Arrival | Scene | Reflection) => {
                unreachable!("common events have a separate surface")
            }
        }
    }

    pub(crate) fn verify_surface(&self, text: &str) -> Result<()> {
        let accepted = text.strip_prefix(LABEL).is_some_and(|body| {
            body.ends_with('。')
                && body.matches('。').count() == usize::from(self.draft.sentence_count)
                && !body.contains(['\r', '\n'])
                && body.contains(&self.protagonist)
                && body.contains(&self.setting)
                && self.elements.iter().all(|element| body.contains(element))
        });
        if !accepted {
            return Err(failure(
                "FICTION_SURFACE",
                "創作表示・設定・文数を保つ文面を生成できませんでした。",
            ));
        }
        Ok(())
    }
}

fn literal(value: &str) -> MeaningGrammarFragment {
    MeaningGrammarFragment::Literal {
        value: value.into(),
    }
}

fn exact(value: &str) -> MeaningGrammarFragment {
    MeaningGrammarFragment::ExactField {
        value: value.into(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GeneratedFictionArtifact {
    kind: String,
    pub(crate) meaning_plan: FictionMeaningPlan,
    pub(crate) content_generation: MeaningGenerationReport,
}

impl GeneratedFictionArtifact {
    pub(crate) fn new(
        meaning_plan: FictionMeaningPlan,
        content_generation: MeaningGenerationReport,
    ) -> Self {
        Self {
            kind: ARTIFACT_KIND.into(),
            meaning_plan,
            content_generation,
        }
    }
}
