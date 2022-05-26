use crate::{note::Note, person::MyUser};
use activitystreams_kinds::activity::{AcceptType, CreateType, FollowType};
use lemmy_apub_lib::{
  data::Data,
  deser::deserialize_one_or_many,
  object_id::ObjectId,
  traits::ActivityHandler,
};
use lemmy_utils::LemmyError;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Deserialize, Serialize, new)]
#[serde(rename_all = "camelCase")]
pub struct Follow {
  pub(crate) actor: ObjectId<MyUser>,
  pub(crate) object: ObjectId<MyUser>,
  #[serde(rename = "type")]
  #[new(default)]
  kind: FollowType,
  id: Url,
}

#[derive(Deserialize, Serialize, new)]
#[serde(rename_all = "camelCase")]
struct Accept {
  actor: ObjectId<MyUser>,
  object: Follow,
  #[serde(rename = "type")]
  #[new(default)]
  kind: AcceptType,
  id: Url,
}

#[derive(Deserialize, Serialize, new)]
#[serde(rename_all = "camelCase")]
pub struct CreateNote {
  pub(crate) actor: ObjectId<MyUser>,
  #[serde(deserialize_with = "deserialize_one_or_many")]
  pub(crate) to: Vec<Url>,
  pub(crate) object: Note,
  #[serde(rename = "type")]
  #[new(default)]
  pub(crate) kind: CreateType,
  pub(crate) id: Url,
}

#[async_trait::async_trait(?Send)]
impl ActivityHandler for Follow {
  type DataType = ();

  fn id(&self) -> &Url {
    &self.id
  }

  fn actor(&self) -> &Url {
    self.actor.inner()
  }

  async fn verify(
    &self,
    _data: &Data<Self::DataType>,
    _request_counter: &mut i32,
  ) -> Result<(), LemmyError> {
    todo!()
  }

  async fn receive(
    self,
    _data: &Data<Self::DataType>,
    _request_counter: &mut i32,
  ) -> Result<(), LemmyError> {
    todo!()
  }
}

#[async_trait::async_trait(?Send)]
impl ActivityHandler for Accept {
  type DataType = ();

  fn id(&self) -> &Url {
    &self.id
  }

  fn actor(&self) -> &Url {
    self.actor.inner()
  }

  async fn verify(
    &self,
    _data: &Data<Self::DataType>,
    _request_counter: &mut i32,
  ) -> Result<(), LemmyError> {
    todo!()
  }

  async fn receive(
    self,
    _data: &Data<Self::DataType>,
    _request_counter: &mut i32,
  ) -> Result<(), LemmyError> {
    todo!()
  }
}

#[async_trait::async_trait(?Send)]
impl ActivityHandler for CreateNote {
  type DataType = ();

  fn id(&self) -> &Url {
    &self.id
  }

  fn actor(&self) -> &Url {
    self.actor.inner()
  }

  async fn verify(
    &self,
    _data: &Data<Self::DataType>,
    _request_counter: &mut i32,
  ) -> Result<(), LemmyError> {
    todo!()
  }

  async fn receive(
    self,
    _data: &Data<Self::DataType>,
    _request_counter: &mut i32,
  ) -> Result<(), LemmyError> {
    todo!()
  }
}
