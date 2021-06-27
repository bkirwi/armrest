use armrest::ml;
use armrest::ml::{Recognizer, Spline, LanguageModel};
use std::io;
use std::io::prelude::*;
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
struct Dict(BTreeSet<String>);

impl Dict {
    const VALID: f32 = 1.0;
    // Tradeoff: you want this to be small, since any plausible input
    // is likely to do something more useful than one the game doesn't understand.
    // However! If a word is not in the dictionary, then choosing a totally
    // implausible word quite far from the input may make the recognizer seem
    // worse than it is.
    // The right value here will depend on both the quality of the model,
    // dictionary size, and some more subjective things.
    const INVALID: f32 = 0.001;

    fn contains_prefix(&self, prefix: &String) -> bool {
        self.0.range::<String, _>(prefix..).next().map_or(false, |c| c.starts_with(prefix))
    }
}

impl LanguageModel for &Dict {
    fn odds(&self, input: &str, ch: char) -> f32 {
        let Dict(words) = self;

        // TODO: use the real lexing rules from https://inform-fiction.org/zmachine/standards/z1point1/sect13.html
        if !ch.is_ascii_lowercase() && !(" .,".contains(ch)) {
            return Dict::INVALID;
        }

        let word_start =
            input.rfind(|c| " .,".contains(c)).map(|i| i + 1).unwrap_or(0);

        let prefix = &input[word_start..];

        // The dictionary only has the first six characters of each word!
        if prefix.len() >= 6 {
            return Dict::VALID;
        }

        // If the current character is punctuation, we check that the prefix is a valid word
        if " .,".contains(ch) {
            return if words.contains(prefix) || prefix.is_empty() { Dict::VALID } else { Dict::INVALID };
        }

        let mut prefix_string = prefix.to_string();
        if self.contains_prefix(&prefix_string) {
            prefix_string.push(ch);
            if self.contains_prefix(&prefix_string) {
                Dict::VALID
            } else {
                Dict::INVALID
            }
        } else {
            Dict::VALID
        }
    }

    fn odds_end(&self, prefix: &str) -> f32 {
        self.odds(prefix, ' ')
    }
}

fn main() {
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    let mut recognizer: Recognizer<Spline> = ml::Recognizer::new().unwrap();

    // This is a dictionary dump
    let word_list: BTreeSet<String> = "analgesic berzio scroll fizmo portraits wizard kulcad castle jug robe oven rezrov turtle pocket foundation brook sign gown lie bulkhead ambassador brochure add sleep bed nitfol  ,        .        a        across   activa   advent   advert   again    air      air-p    all      altar    an       ancien   and      answer   antiqu   apply    around   art      ask      at       attach   attack   aviato   awake    away     ax       axe      back     bag      banish   bar      bare     barf     barrow   basket   bat      bathe    bauble   beauti   beetle   begone   behind   bell     below    beneat   bird     birds    bite     black    blade    blast    blessi   block    bloody   blow     blue     board    boarde   boards   boat     bodies   body     bolt     bones    book     bookle   books    bottle   box      bracel   branch   brandi   brass    break    breath   brief    broken   brown    brush    bubble   bug      buoy     burn     burned   burnin   but      button   cage     canary   candle   canvas   carpet   carry    carved   case     casket   cast     catch    chain    chalic   chant    chase    chasm    chest    chests   chimne   chomp    chuck    chute    clean    clear    cliff    cliffs   climb    clockw   close    clove    coal     coffin   coil     coins    coloni   come     comman   consum   contai   contro   count    cover    crack    crawlw   cretin   cross    crysta   cup      curse    cut      cyclop   d        dam      damage   damn     dark     dead     deflat   derang   descri   destro   diagno   diamon   dig      dinner   dirt     disemb   disenc   dispat   dive     dome     donate   door     douse    down     drink    drip     drive    driver   drop     dryer    dumbwa   dusty    e        east     eat      echo     egg      egypti   elonga   elvish   emeral   enamel   enchan   encrus   engrav   enormo   enter    evil     examin   except   exit     exorci   exquis   exting   eye      fall     fantas   fasten   fcd#     fear     feeble   feed     feel     fence    fermen   fiends   fierce   fight    figuri   filch    fill     find     fine     finepr   firepr   fix      flamin   flathe   flip     float    floor    fluore   follow   foobar   food     footpa   for      forbid   force    ford     forest   fork     free     freeze   frigid   froboz   from     front    frotz    fry      fuck     fudge    fumble   g        garlic   gas      gate     gates    gaze     get      ghosts   giant    give     glamdr   glass    glue     go       gold     golden   gothic   grab     graces   granit   grate    gratin   grease   green    ground   group    grue     guide    guideb   gunk     h2o      hand     hand-    hands    hatch    head     heap     hello    hemloc   hemp     her      here     hi       hide     him      hit      hold     hop      hot      house    huge     hungry   hurl     hurt     i        ignite   imbibe   impass   in       incant   incine   inflat   injure   inscri   insert   inside   intnum   into     invent   invisi   is       it       ivory    jade     jewel    jewele   jewels   jump     key      kick     kill     kiss     kitche   knife    knives   knock    l        label    ladder   lake     lamp     land     lanter   large    launch   leaf     leafle   leak     lean     leap     leathe   leave    leaves   ledge    letter   lid      lift     light    liquid   liquif   listen   lock     long     look     lose     lower    lowere   lubric   lunch    lungs    lurkin   machin   magic    mail     mailbo   make     man      mangle   manual   map      marble   massiv   match    matchb   matche   materi   me       melt     metal    mirror   molest   monste   mounta   mouth    move     mumble   murder   myself   n        nail     nails    narrow   nasty    ne       nest     no       north    northe   northw   nut      nw       odor     odysse   of       off      offer    oil      old      on       one      onto     open     orcris   orient   out      over     overbo   own      owners   ozmoo    page     paint    painti   pair     panel    paper    parchm   passag   paste    pat      patch    path     pdp1     peal     pedest   pepper   person   pet      pick     piece    pierce   pile     pines    pipe     place    plasti   platin   play     plug     plugh    poke     poseid   pot      pour     pray     prayer   press    print    procee   pull     pump     punctu   pursue   push     put      q        quanti   quit     raft     rail     railin   rainbo   raise    ramp     range    rap      rape     read     red      reflec   releas   remain   remove   repair   repent   reply    restar   restor   ricket   ring     river    robber   rocky    roll     rope     rub      rug      run      rusty    s        sack     sailor   sand     sandwi   sapphi   save     say      scarab   scepte   sceptr   score    scream   screw    screwd   script   se       search   seawor   secure   see      seedy    seek     self     send     set      shady    shake    sharp    sheer    shit     shout    shovel   shut     sigh     silent   silver   sinist   sit      skelet   skim     skip     skull    slag     slay     slice    slide    small    smash    smell    smelly   sniff    solid    song     songbi   south    southe   southw   spill    spin     spirit   spray    squeez   stab     stairc   stairs   stairw   stand    stare    startl   stay     steep    step     steps    stilet   stone    storm    strang   stream   strike   stuff    super    superb   surpri   surrou   suspic   sw       swallo   swim     swing    switch   sword    table    take     talk     tan      taste    taunt    teeth    tell     temple   the      them     then     thief    thiefs   throug   throw    thru     thrust   tie      timber   to       tomb     tool     toolch   tools    tooth    torch    toss     touch    tour     trail    trap     trap-    trapdo   treasu   tree     trees    triden   troll    trophy   trunk    tube     tug      turn     twisti   u        ulysse   unatta   under    undern   unfast   unhook   unlock   unrust   unscri   untie    up       useles   using    valve    vampir   verbos   versio   viciou   viscou   vitreo   w        wade     wait     wake     walk     wall     walls    water    wave     wear     west     what     whats    where    white    win      wind     windin   window   winnag   wish     with     wooden   wrench   writin   xyzzy    y        yank     yell     yellow   yes      z        zork     zorkmi   zzmgck".split_ascii_whitespace().map(|w| w.to_string()).collect();
    let lm = Dict(word_list);

    let mut global_err = 0f32;
    let mut count = 0;
    while let Some(Ok(line)) = lines.next() {
        let mut halves = line.split("\t");
        let expected = halves.next().unwrap();
        let points = halves.next().unwrap();

        let mut ink = vec![];
        for point_str in points.split(",") {
            for part in point_str.split(" ") {
                ink.push(part.parse::<f32>().unwrap());
            }
        }

        let actual = recognizer.recognize(&ink.as_slice(), &ml::Beam { size: 64, language_model: &lm }).unwrap();

        let cer = strsim::levenshtein(expected, &actual[0].0) as f32 / expected.len() as f32;
        global_err += cer;
        count += 1;

        println!("[{:.4} / {:.4}] {} -> {}", cer, actual[0].1, expected, actual[0].0);
    }

    println!("Average CER: {:.4}", global_err / count as f32);
}
