use lucid_core::CLEANUP_INSTRUCTION;

#[test]
fn pinyin_prompt_preserves_repeated_meaningful_words() {
    assert!(CLEANUP_INSTRUCTION.contains("xianzai wozai kankan"));
    assert!(CLEANUP_INSTRUCTION.contains("现在我再看看"));
    assert!(CLEANUP_INSTRUCTION.contains("Do not silently drop"));
    assert!(CLEANUP_INSTRUCTION.contains("Treat the input as an ordered sequence"));
    assert!(CLEANUP_INSTRUCTION.contains("jintian wo bu qu"));
    assert!(CLEANUP_INSTRUCTION.contains("ta ye zai deng wo"));
    assert!(CLEANUP_INSTRUCTION.contains("mingtian ni zai lai"));
    assert!(CLEANUP_INSTRUCTION.contains("youhuaxia zhe shurufa de ui tubiao"));
    assert!(CLEANUP_INSTRUCTION.contains("'youhuaxia' is '优化下'"));
    assert!(CLEANUP_INSTRUCTION.contains("右下角"));
    assert!(CLEANUP_INSTRUCTION.contains("mandatory order for pinyin"));
    assert!(CLEANUP_INSTRUCTION.contains("longest-match word segmentation"));
    assert!(
        CLEANUP_INSTRUCTION
            .contains("adds a fact, location, object, or relationship absent from the input")
    );
    assert!(CLEANUP_INSTRUCTION.contains("wo xiang ba zhege youhua yixia"));
    assert!(CLEANUP_INSTRUCTION.contains("qing ba zheduan wenzi fanyi cheng yingwen"));
    assert!(CLEANUP_INSTRUCTION.contains("kyl' means '可以了"));
    assert!(CLEANUP_INSTRUCTION.contains("bzd' means '不知道"));
    assert!(CLEANUP_INSTRUCTION.contains("wsm' means '为什么"));
    assert!(CLEANUP_INSTRUCTION.contains("Input: kyl."));
    assert!(CLEANUP_INSTRUCTION.contains("Keep established technical or English abbreviations"));
    assert!(CLEANUP_INSTRUCTION.contains("as a separate input class"));
    assert!(CLEANUP_INSTRUCTION.contains("2–6 letters"));
    assert!(CLEANUP_INSTRUCTION.contains("bzd."));
    assert!(CLEANUP_INSTRUCTION.contains("wsm?"));
    assert!(CLEANUP_INSTRUCTION.contains("Never leave raw Chinese pinyin, chat initialisms"));
}

#[test]
fn pinyin_prompt_keeps_input_word_meaning_over_topic_context() {
    assert!(
        CLEANUP_INSTRUCTION
            .contains("The words actually present in the user's input have priority")
    );
    assert!(CLEANUP_INSTRUCTION.contains("nizheg zhongwen shibiede haiyou dian wenti a."));
    assert!(CLEANUP_INSTRUCTION.contains("Your Chinese recognition still has some issues."));
    assert!(CLEANUP_INSTRUCTION.contains("你这个中文识别的还有点问题啊。"));
    assert!(CLEANUP_INSTRUCTION.contains("'识别' (recognition), not '输入法' (input method)"));
    assert!(CLEANUP_INSTRUCTION.contains("zhege shibie haiyou wenti."));
    assert!(CLEANUP_INSTRUCTION.contains("This recognition still has some issues."));
    assert!(CLEANUP_INSTRUCTION.contains("Allow small typing imperfections"));
    assert!(CLEANUP_INSTRUCTION.contains("Apply a final lexical evidence check"));
    assert!(
        CLEANUP_INSTRUCTION.contains("'shibie' supports '识别' and 'shurufa' supports '输入法'")
    );
    assert!(CLEANUP_INSTRUCTION.contains("zhege shibie bu shi shurufa."));
    assert!(CLEANUP_INSTRUCTION.contains("zhege shurufa haiyou wenti."));
    assert!(CLEANUP_INSTRUCTION.contains("Do not use the examples or rules in this instruction"));
}
