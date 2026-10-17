import pytest
from .macos import Reference,case_catalog


def test_reference_target_is_exact_not_a_suffix_of_another_control():
    reference=object.__new__(Reference)
    reference.state='2 button 6, ID: button-6\n18 button Record 6\n7 text entry area (settable) Description: Draft, Value: seed\n12 scroll area Description: Records, Secondary Actions: Scroll Up'
    assert reference.target('6')==2
    assert reference.target('Record 6')==18
    assert reference.target('Draft')==7
    assert reference.target('Records')==12
    reference.state+='\n14 Markdown'
    assert reference.target('Markdown')==14
    reference.state+='\n22 button 6'
    with pytest.raises(RuntimeError,match='not unique'):reference.target('6')


def test_expanded_catalog_preserves_unrelated_document_in_every_case():
    cases=case_catalog('expanded')
    assert len(cases)==12
    assert all(c['expected']['Alpha']=='alpha' for c in cases.values())
    assert '\n' in cases['multiline_text']['expected']['draft']
    assert cases['scroll_reveal']['expected']['scrolled'] is True
