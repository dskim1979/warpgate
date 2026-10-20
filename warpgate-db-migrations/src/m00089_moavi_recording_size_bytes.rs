use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// MOAVI: recordings remember their size in storage (bytes, all files). Null for rows from
/// before, and while a recording is in progress; filled when it ends or on first read.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("recordings"))
                    .add_column(ColumnDef::new(Alias::new("size_bytes")).big_integer().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("recordings"))
                    .drop_column(Alias::new("size_bytes"))
                    .to_owned(),
            )
            .await
    }
}
