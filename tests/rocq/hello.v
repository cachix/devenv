(* hello.v *)
Theorem hello : forall A B : Prop, A /\ B -> B /\ A.
Proof.
  intros A B H. split; destruct H; assumption.
Qed.
