from trajectory_verdict import memory_decisions


def test_system_marker_is_not_proof_of_reuse():
    calls=[{'request':{'messages':[{'role':'system','content':'MEMORY <unverified_notes>'},
                                  {'role':'user','content':'WINDOW ELEMENT TREE: buttons'}]}}]
    assert memory_decisions(calls,[{'text':'multiply'}])==0


def test_actual_scoped_note_must_reach_user_prompt():
    calls=[{'request':{'messages':[{'role':'user','content':'WINDOW ELEMENT TREE: buttons\nMEMORY\n<unverified_notes>\n{"note":"multiply"}\n</unverified_notes>'}]}}]
    assert memory_decisions(calls,[{'text':'multiply'}])==1
    assert memory_decisions(calls,[{'text':'different note'}])==0
