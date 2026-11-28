TEST(operation, geocentricCRS_to_geocentricCRS_different_datum_context) {
    auto authFactory =
        AuthorityFactory::create(DatabaseContext::create(), "EPSG");
    auto ctxt = CoordinateOperationContext::create(authFactory, nullptr, 0.0);
    auto list = CoordinateOperationFactory::create()->createOperations(
        // ITRF2000 (geocentric)
        authFactory->createCoordinateReferenceSystem("4919"),
        // ITRF2005 (geocentric)
        authFactory->createCoordinateReferenceSystem("4896"), ctxt);
    ASSERT_EQ(list.size(), 1U);
    EXPECT_EQ(list[0]->nameStr(), "ITRF2000 to ITRF2005 (1)");
    EXPECT_PRED_FORMAT2(
        ComparePROJString,
        list[0]->exportToPROJString(PROJStringFormatter::create().get()),
        "+proj=helmert +x=-0.0001 "
        "+y=0.0008 +z=0.0058 +rx=0 +ry=0 +rz=0 +s=-0.0004 +dx=0.0002 "
        "+dy=-0.0001 +dz=0.0018 +drx=0 +dry=0 +drz=0 +ds=-8e-05 "
        "+t_epoch=2000 +convention=position_vector");
}

